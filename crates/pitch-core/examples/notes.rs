//! Dev tool: analyse a WAV into notes, write the pitch track and notes as CSV, and an
//! unedited render for comparison. `cargo run --example notes -- in.wav out_prefix [range]`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solplugs_pitch_core::notes::{Formant, NoteEdits, analyse, render};
use solplugs_pitch_core::realtime::Range;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut r = hound::WavReader::open(&args[0]).unwrap();
    let spec = r.spec();
    let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
    let x: Vec<f32> = r
        .samples::<i32>()
        .map(|s| s.unwrap() as f32 * scale)
        .collect();
    let sr = spec.sample_rate as f32;
    let range = match args.get(2).map(String::as_str) {
        Some("high") => Range::High,
        Some("low") => Range::Low,
        _ => Range::Mid,
    };
    let an = analyse(&x, sr, range);
    let e = NoteEdits::new(&an);
    let mut t = String::from("t,midi,rms\n");
    for (i, f) in an.frames.iter().enumerate() {
        t += &format!("{},{},{}\n", (i * an.hop) as f32 / sr, f.midi, f.rms);
    }
    std::fs::write(format!("{}-track.csv", args[1]), t).unwrap();
    let mut n = String::from("start,end,pitch\n");
    for note in &e.notes {
        n += &format!(
            "{},{},{}\n",
            note.src_start as f32 / sr,
            note.src_end as f32 / sr,
            note.pitch
        );
    }
    std::fs::write(format!("{}-notes.csv", args[1]), n).unwrap();
    let y = render(
        &[&x],
        &an,
        &e,
        Formant {
            preserve: true,
            shift: 0.0,
        },
    )
    .remove(0);
    let (mut worst, mut at) = (0f32, 0);
    for i in 0..x.len() {
        if (x[i] - y[i]).abs() > worst {
            worst = (x[i] - y[i]).abs();
            at = i;
        }
    }
    println!(
        "unedited render max error {worst:e} at {:.4}s",
        at as f32 / sr
    );
    let near: Vec<_> = an
        .marks
        .iter()
        .filter(|m| (m.pos - at as i64).abs() < 600)
        .collect();
    println!("{near:?}");
}
