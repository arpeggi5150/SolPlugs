//! Engine end to end: live processing, graph capture through a fake transport, a note
//! edit played back at the right timeline position, and state save/load.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use solplugs_pitch_app::engine::{Engine, GraphStatus, Transport};
use solplugs_pitch_app::params::P;

fn load(name: &str) -> (Vec<f32>, f32) {
    let path = format!(
        "{}/../pitch-core/testdata/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut r = hound::WavReader::open(path).unwrap();
    let sr = r.spec().sample_rate as f32;
    let x = r
        .samples::<i32>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    (x, sr)
}

/// Plays `x` (stereo copies) through the processor from timeline position `at`.
fn play(engine: &Arc<Engine>, x: &[f32], sr: f32, at: i64, playing: bool) -> Vec<f32> {
    let mut p = engine.processor(f64::from(sr), 512);
    let mut out = Vec::with_capacity(x.len());
    let mut pos = 0;
    while pos < x.len() {
        let n = 512.min(x.len() - pos);
        let mut l = x[pos..pos + n].to_vec();
        let mut r = l.clone();
        p.process(
            &mut [&mut l, &mut r],
            Transport {
                playing,
                pos: Some(at + pos as i64),
            },
        );
        out.extend_from_slice(&l);
        pos += n;
        // Faster than real time, but not so fast that the capture ring (about 5 s)
        // overflows before the worker drains it, as a real host never would.
        if (pos / 512) % 64 == 0 {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    out
}

fn wait_for(engine: &Engine, status: GraphStatus) {
    let t = Instant::now();
    while engine.graph().status != status {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "stuck at {:?}",
            engine.graph().status
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn capture_edit_and_play_back() {
    let (x, sr) = load("singing-female.wav");
    let engine = Arc::new(Engine::new());
    engine.set(P::Mode, 1.0);
    engine.arm_capture(true);
    let at = 96_000;
    let mut padded = x.clone();
    padded.extend(std::iter::repeat_n(0.0, 8192));
    play(&engine, &padded, sr, at, true);
    // Transport stops: the take is analysed.
    play(&engine, &[0.0; 2048], sr, at, false);
    wait_for(&engine, GraphStatus::Ready);
    let lat = engine.latency() as usize;
    {
        let g = engine.graph();
        let take = g.take.as_ref().unwrap();
        assert_eq!(take.start, at);
        assert!(take.len() >= x.len(), "take {} < {}", take.len(), x.len());
        assert_eq!(&take.channels[0][..x.len()], &x[..]);
        assert!(g.edits.notes.len() >= 3, "{} notes", g.edits.notes.len());
    }
    // Unedited playback in graph mode is the take itself (here: the same audio).
    let y = play(&engine, &padded, sr, at, true);
    let err = (lat..x.len())
        .map(|t| (y[t] - x[t - lat]).abs())
        .fold(0f32, f32::max);
    assert!(err < 1e-4, "unedited playback error {err}");
    // Drag the first note up two semitones: playback changes inside it only.
    let note = {
        let mut g = engine.graph();
        g.checkpoint();
        g.edits.set_shift(0, 2.0);
        g.touch();
        g.edits.notes[0]
    };
    std::thread::sleep(Duration::from_millis(300));
    // Feed silence: what comes out is the take, not the input.
    let silent = vec![0.0; padded.len()];
    let y = play(&engine, &silent, sr, at, true);
    let mid = (note.src_start + note.src_end) as usize / 2;
    let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
    assert!(
        rms(&y[mid + lat..mid + lat + 4096]) > 0.01,
        "take not playing"
    );
    // Undo restores the original.
    {
        let mut g = engine.graph();
        g.undo();
        assert_eq!(g.edits.notes[0].shift, 0.0);
    }
    // State round trip keeps the take and edits.
    {
        let mut g = engine.graph();
        g.edits.set_shift(1, -1.0);
        g.touch();
    }
    let state = engine.save_state();
    let other = Arc::new(Engine::new());
    assert!(other.load_state(&state));
    assert!(!other.load_state(&[]));
    assert!(!other.load_state(b"random bytes that are not a state"));
    wait_for(&other, GraphStatus::Ready);
    let g = other.graph();
    assert_eq!(g.take.as_ref().unwrap().start, at);
    assert_eq!(g.edits.notes[1].shift, -1.0);
    assert!(other.params().on(P::Mode));
}

#[test]
fn live_path_reports_latency_and_bypasses_exactly() {
    let (x, sr) = load("vignesh.wav");
    let engine = Arc::new(Engine::new());
    engine.set(P::Bypass, 1.0);
    let y = play(&engine, &x, sr, 0, true);
    let lat = engine.latency() as usize;
    let ms = lat as f32 * 1000.0 / sr;
    println!("latency {lat} samples = {ms:.1} ms at {sr} Hz (mid range)");
    let skip = (0.02 * sr) as usize;
    assert_eq!(&y[lat + skip..], &x[skip..x.len() - lat]);
    for (range, name) in [(0.0, "high"), (2.0, "low")] {
        engine.set(P::Range, range);
        play(&engine, &x[..4096], sr, 0, true);
        let l = engine.latency();
        println!(
            "{name} range latency {l} samples = {:.1} ms",
            l as f32 * 1000.0 / sr
        );
    }
}
