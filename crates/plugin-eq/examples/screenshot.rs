//! Dev tool: opens the plugin editor (the same baseview window a host gets) in its own
//! window with a vocal looping through the engine, for checking the UI without a DAW.
//! `cargo run --example screenshot -- [pultec|api|ssl|neve|clean|massive|maag|curve] [seconds]`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use solplugs_eq_app::engine::Engine;
use solplugs_eq_core::Style;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let style = args
        .first()
        .and_then(|a| {
            Style::ALL
                .into_iter()
                .find(|s| s.name().to_lowercase() == a.to_lowercase())
        })
        .unwrap_or(Style::Clean);
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
    engine.set_style(style);
    // Feed the vocal in real time so the meters and waveforms move.
    let e = engine.clone();
    std::thread::spawn(move || {
        let mut p = e.processor(f64::from(sr), 512);
        let mut pos = 0usize;
        loop {
            let at = pos % (x.len() - 512);
            let mut l = x[at..at + 512].to_vec();
            let mut r = l.clone();
            p.process(&mut [&mut l, &mut r]);
            pos += 512;
            std::thread::sleep(Duration::from_secs_f32(512.0 / sr));
        }
    });
    let w = solplugs_eq::editor::open_standalone(engine).unwrap();
    let _ = w.show();
    std::thread::sleep(Duration::from_secs(secs));
}
