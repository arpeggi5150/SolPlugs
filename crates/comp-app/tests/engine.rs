//! Engine behaviour: per-mode memory, A/B, presets, MATCH, auto-makeup, state, sidechain.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use solplugs_comp_app::engine::{Engine, HostEvent};
use solplugs_comp_app::params::{G, K, Pid};
use solplugs_comp_app::presets::PRESETS;
use solplugs_comp_core::Mode;

const SR: f64 = 48_000.0;

fn drums(seconds: f32) -> Vec<f32> {
    let n = (seconds * 48_000.0) as usize;
    let beat = 24_000;
    let mut seed = 1u32;
    (0..n)
        .map(|i| {
            let t = (i % beat) as f32 / 48_000.0;
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            if (i / beat) % 2 == 0 {
                let f = 50.0 + 70.0 * (-t * 30.0).exp();
                0.9 * (std::f32::consts::TAU * f * t).sin() * (-t * 9.0).exp()
            } else {
                0.6 * noise * (-t * 22.0).exp()
            }
        })
        .collect()
}

fn play(engine: &Arc<Engine>, x: &[f32]) -> Vec<f32> {
    let mut p = engine.processor(SR, 512);
    let mut l = x.to_vec();
    let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
        p.process(&mut [a, b], None);
    }
    l
}

#[test]
fn every_mode_remembers_its_own_settings() {
    let e = Engine::new();
    e.set_mode(Mode::Fet);
    e.set(Pid::m(Mode::Fet, K::Attack), 0.05);
    e.set(Pid::m(Mode::Fet, K::AllButtons), 1.0);
    e.set_mode(Mode::Opto);
    e.set(Pid::m(Mode::Opto, K::Threshold), -30.0);
    e.set_mode(Mode::Fet);
    let fet = e.params().settings(Mode::Fet);
    assert!((fet.attack_ms - 0.05).abs() < 1e-6);
    assert!(fet.all_buttons);
    assert_eq!(e.params().settings(Mode::Opto).threshold_db, -30.0);
}

#[test]
fn ui_edits_reach_the_host() {
    let e = Engine::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    e.set_listener(Some(Arc::new(move |ev| s.lock().unwrap().push(ev))));
    e.set(Pid::m(Mode::Vca, K::Ratio), 8.0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(
        matches!(seen[1], HostEvent::Value(p, v) if p == Pid::m(Mode::Vca, K::Ratio) && v == 8.0)
    );
}

#[test]
fn ab_switches_whole_setups() {
    let e = Engine::new();
    e.set(Pid::m(Mode::Vca, K::Threshold), -10.0);
    e.select_ab(1);
    // B starts as a copy of A.
    assert_eq!(e.params().m(Mode::Vca, K::Threshold), -10.0);
    e.set_mode(Mode::VariMu);
    e.set(Pid::m(Mode::VariMu, K::Ratio), 2.0);
    e.select_ab(0);
    assert_eq!(e.mode(), Mode::Vca);
    e.select_ab(1);
    assert_eq!(e.mode(), Mode::VariMu);
    assert_eq!(e.params().m(Mode::VariMu, K::Ratio), 2.0);
}

#[test]
fn presets_load_and_cover_the_brief() {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    for want in [
        "Vocals",
        "Drums",
        "Bass",
        "Mix bus",
        "Mastering",
        "Parallel smash",
    ] {
        assert!(names.contains(&want), "{want}");
    }
    for m in Mode::ALL {
        assert!(PRESETS.iter().any(|p| p.mode == m), "{m:?}");
    }
    let e = Engine::new();
    let i = names.iter().position(|n| *n == "Parallel smash").unwrap();
    e.load_preset(i);
    assert_eq!(e.mode(), Mode::Fet);
    let s = e.params().settings(Mode::Fet);
    assert!(s.all_buttons && (s.mix - 0.4).abs() < 1e-6);
}

#[test]
fn match_button_lines_up_before_and_after() {
    let e = Arc::new(Engine::new());
    e.set(Pid::m(Mode::Vca, K::Threshold), -28.0);
    let x = drums(6.0);
    play(&e, &x);
    let gap = e.meters().avg_before() - e.meters().avg_after();
    assert!(gap > 3.0, "{gap}");
    e.match_now();
    play(&e, &x);
    let gap = e.meters().avg_before() - e.meters().avg_after();
    println!(
        "after MATCH: before {:.2} after {:.2}",
        e.meters().avg_before(),
        e.meters().avg_after()
    );
    assert!(gap.abs() < 0.5, "{gap}");
}

#[test]
fn auto_makeup_turning_off_keeps_the_gain() {
    let e = Arc::new(Engine::new());
    e.set(Pid::m(Mode::Vca, K::Threshold), -28.0);
    e.set_auto_makeup(true);
    play(&e, &drums(6.0));
    let auto = e.meters().auto_makeup();
    assert!(auto > 3.0, "{auto}");
    e.set_auto_makeup(false);
    let g = e.params().m(Mode::Vca, K::GainOut) as f32;
    assert!((g - auto).abs() < 0.05, "{g} vs {auto}");
}

#[test]
fn state_round_trips_and_rejects_junk() {
    let e = Engine::new();
    e.set_mode(Mode::Clean);
    e.set(Pid::m(Mode::Clean, K::Lookahead), 3.0);
    e.set(Pid::g(G::Link), 0.0);
    e.select_ab(1);
    let data = e.save_state();
    let f = Engine::new();
    assert!(f.load_state(&data));
    assert_eq!(f.mode(), Mode::Clean);
    assert_eq!(f.params().m(Mode::Clean, K::Lookahead), 3.0);
    assert!(!f.params().global().link);
    assert_eq!(f.ab(), 1);
    assert!(!f.load_state(b""));
    assert!(!f.load_state(b"garbage"));
}

#[test]
fn latency_follows_the_mode() {
    let e = Arc::new(Engine::new());
    let mut p = e.processor(SR, 512);
    let mut buf = vec![0.0f32; 512];
    p.process(&mut [&mut buf], None);
    assert_eq!(p.latency(), 28);
    e.take_latency_changed();
    e.set_mode(Mode::Clean);
    p.process(&mut [&mut buf], None);
    assert_eq!(p.latency(), 0);
    assert!(e.take_latency_changed());
    for m in Mode::ALL {
        println!("latency {}: {:.2} ms", m.name(), e.latency_ms(m));
    }
}

#[test]
fn external_sidechain_drives_the_detector() {
    let e = Arc::new(Engine::new());
    e.set(Pid::g(G::ExtSc), 1.0);
    e.set_mode(Mode::Clean);
    let mut p = e.processor(SR, 512);
    // Quiet main input, loud key: the key must cause gain reduction.
    let mut l = vec![0.01f32; 4800];
    let mut r = l.clone();
    let key = vec![0.9f32; 4800];
    p.process(&mut [&mut l, &mut r], Some(&[&key, &key]));
    assert!(e.meters().gr().0 > 6.0, "{:?}", e.meters().gr());
    // Without a key, nothing happens.
    e.set(Pid::g(G::ExtSc), 0.0);
    let mut p = e.processor(SR, 512);
    p.process(&mut [&mut l, &mut r], Some(&[&key, &key]));
    assert!(e.meters().gr().0 < 0.1);
}

#[test]
fn display_columns_flow() {
    let e = Arc::new(Engine::new());
    play(&e, &drums(1.0));
    let mut n = 0;
    let mut gr = 0.0f32;
    e.drain_columns(|c| {
        n += 1;
        gr = gr.max(c.gr);
        assert!(c.before.0 <= c.before.1 && c.after.0 <= c.after.1);
    });
    assert!((195..=201).contains(&n), "{n}");
    assert!(gr > 1.0);
}
