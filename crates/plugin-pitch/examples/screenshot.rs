//! Dev tool: opens the editor in its own window with a vocal playing through the engine,
//! for looking at the UI without a DAW. `cargo run --example screenshot -- [auto|graph] [seconds]`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use solplugs_pitch_app::engine::{Engine, GraphStatus, Transport};
use solplugs_pitch_app::params::P;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let graph = args.first().is_some_and(|a| a == "graph");
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pitch-core/testdata/singing-female.wav"
    );
    let mut r = hound::WavReader::open(path).unwrap();
    let sr = r.spec().sample_rate as f32;
    let x: Vec<f32> = r
        .samples::<i32>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    let engine = Arc::new(Engine::new());
    engine.set(P::Key, 8.0); // G#
    engine.set(P::Scale, 1.0); // major
    if graph {
        engine.set(P::Mode, 1.0);
        engine.arm_capture(true);
        let mut p = engine.processor(f64::from(sr), 512);
        for (i, c) in x.chunks(512).enumerate() {
            let mut b = c.to_vec();
            p.process(
                &mut [&mut b],
                Transport {
                    playing: true,
                    pos: Some(i as i64 * 512),
                },
            );
        }
        p.process(
            &mut [&mut [0.0; 512]],
            Transport {
                playing: false,
                pos: Some(0),
            },
        );
        let t = Instant::now();
        while engine.graph().status != GraphStatus::Ready && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut g = engine.graph();
        g.edits.set_shift(1, 1.0);
        let n = g.edits.notes[3];
        g.edits.set_end(3, n.dst_end + (0.15 * sr) as i64, sr);
        g.touch();
    }
    // Feed the vocal in real time so the meters and trail move.
    let e = engine.clone();
    std::thread::spawn(move || {
        let mut p = e.processor(f64::from(sr), 512);
        let mut pos = 0usize;
        loop {
            let c = &x[pos % (x.len() - 512)..pos % (x.len() - 512) + 512];
            let mut b = c.to_vec();
            p.process(
                &mut [&mut b],
                Transport {
                    playing: true,
                    pos: Some((pos % x.len()) as i64),
                },
            );
            pos += 512;
            std::thread::sleep(Duration::from_secs_f32(512.0 / sr));
        }
    });
    let w = solplugs_pitch::editor::open_standalone(engine).unwrap();
    let _ = w.show();
    std::thread::sleep(Duration::from_secs(secs));
}
