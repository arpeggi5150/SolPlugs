//! Engine behaviour: per-style memory, A/B, presets, MATCH, auto-makeup, Clean bands,
//! state, latency, bypass and the display feeds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use solplugs_eq_app::engine::{Engine, HostEvent};
use solplugs_eq_app::params::{B, C, G, Pid};
use solplugs_eq_app::presets::PRESETS;
use solplugs_eq_core::style::{neve, pultec, ssl};
use solplugs_eq_core::{Shape, Style};

const SR: f64 = 48_000.0;

/// Pink-ish noise with a beat: a stand-in for a mix.
fn music(seconds: f32) -> Vec<f32> {
    let n = (seconds * 48_000.0) as usize;
    let mut seed = 7u32;
    let mut lp = 0.0f32;
    (0..n)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let w = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            lp += 0.08 * (w - lp);
            let t = (i % 24_000) as f32 / 48_000.0;
            let kick = (std::f32::consts::TAU * 60.0 * t).sin() * (-t * 8.0).exp();
            0.25 * w + 0.5 * lp + 0.4 * kick
        })
        .collect()
}

fn play(engine: &Arc<Engine>, x: &[f32]) -> Vec<f32> {
    let mut p = engine.processor(SR, 512);
    let mut l = x.to_vec();
    let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
        p.process(&mut [a, b]);
    }
    l
}

#[test]
fn every_style_remembers_its_own_settings() {
    let e = Engine::new();
    e.set_style(Style::Pultec);
    e.set(Pid::k(Style::Pultec, pultec::LOW_BOOST), 6.0);
    e.set(Pid::c(Style::Pultec, C::GainOut), -2.0);
    e.set_style(Style::Ssl);
    e.set(Pid::k(Style::Ssl, ssl::HMF_GAIN), -4.0);
    e.set(Pid::c(Style::Ssl, C::GainOut), 1.5);
    e.set_style(Style::Pultec);
    let p = e.params();
    assert_eq!(p.get(Pid::k(Style::Pultec, pultec::LOW_BOOST)), 6.0);
    assert_eq!(p.c(Style::Pultec, C::GainOut), -2.0);
    assert_eq!(p.get(Pid::k(Style::Ssl, ssl::HMF_GAIN)), -4.0);
    assert_eq!(p.c(Style::Ssl, C::GainOut), 1.5);
    // Another style's knobs are untouched.
    assert_eq!(p.get(Pid::k(Style::Neve, neve::LOW_GAIN)), 0.0);
}

#[test]
fn ui_edits_reach_the_host() {
    let e = Engine::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    e.set_listener(Some(Arc::new(move |ev| s.lock().unwrap().push(ev))));
    let id = Pid::k(Style::Api, 1);
    e.set(id, 4.0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(matches!(seen[1], HostEvent::Value(p, v) if p == id && v == 4.0));
}

#[test]
fn ab_switches_whole_setups() {
    let e = Engine::new();
    e.set_style(Style::Neve);
    e.set(Pid::k(Style::Neve, neve::MID_GAIN), 5.0);
    e.select_ab(1);
    // B starts as a copy of A.
    assert_eq!(e.params().get(Pid::k(Style::Neve, neve::MID_GAIN)), 5.0);
    e.set_style(Style::Clean);
    let b = e.add_band(Shape::Bell, 2000.0, -6.0, 2.0).unwrap();
    e.select_ab(0);
    assert_eq!(e.style(), Style::Neve);
    assert!(!e.params().on(Pid::b(b, B::Used)));
    e.select_ab(1);
    assert_eq!(e.style(), Style::Clean);
    assert!(e.params().on(Pid::b(b, B::Used)));
    assert_eq!(e.params().get(Pid::b(b, B::Gain)), -6.0);
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
        "Air / sparkle",
        "Low-end clean-up",
    ] {
        assert!(names.contains(&want), "{want}");
    }
    for s in Style::ALL {
        assert!(PRESETS.iter().any(|p| p.style == s), "{s:?}");
    }
    let e = Arc::new(Engine::new());
    let x = music(1.0);
    for (i, p) in PRESETS.iter().enumerate() {
        e.load_preset(i);
        assert_eq!(e.style(), p.style, "{}", p.name);
        assert_eq!(e.preset(), Some(i));
        let y = play(&e, &x);
        assert!(y.iter().all(|v| v.is_finite()), "{}", p.name);
    }
}

#[test]
fn match_button_lines_up_before_and_after() {
    let e = Arc::new(Engine::new());
    e.set_style(Style::Neve);
    e.set(Pid::k(Style::Neve, neve::LOW_GAIN), 10.0);
    e.set(Pid::k(Style::Neve, neve::HIGH_GAIN), 8.0);
    let x = music(6.0);
    play(&e, &x);
    let gap = e.meters().avg_before() - e.meters().avg_after();
    assert!(gap < -3.0, "{gap}");
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
fn auto_makeup_follows_and_turning_off_keeps_the_gain() {
    let e = Arc::new(Engine::new());
    e.set_style(Style::Ssl);
    e.set(Pid::k(Style::Ssl, ssl::LF_GAIN), 12.0);
    e.set_auto_makeup(true);
    play(&e, &music(6.0));
    let auto = e.meters().auto_makeup();
    assert!(auto < -3.0, "{auto}");
    let gap = e.meters().avg_before() - e.meters().avg_after();
    assert!(gap.abs() < 1.0, "{gap}");
    e.set_auto_makeup(false);
    let g = e.params().c(Style::Ssl, C::GainOut) as f32;
    assert!((g - auto).abs() < 0.05, "{g} vs {auto}");
}

#[test]
fn clean_has_twenty_four_bands_and_no_more() {
    let e = Engine::new();
    e.set_style(Style::Clean);
    for i in 0..24 {
        assert_eq!(e.add_band(Shape::Bell, 100.0 + i as f64 * 500.0, 1.0, 1.0), Some(i));
    }
    assert_eq!(e.add_band(Shape::Bell, 1000.0, 1.0, 1.0), None);
    e.remove_band(5);
    assert_eq!(e.add_band(Shape::Notch, 1000.0, 0.0, 8.0), Some(5));
    e.solo_band(3, true);
    assert!(e.params().on(Pid::b(3, B::Solo)));
    e.solo_band(7, true);
    assert!(!e.params().on(Pid::b(3, B::Solo)) && e.params().on(Pid::b(7, B::Solo)));
}

#[test]
fn state_round_trips_and_rejects_junk() {
    let e = Engine::new();
    e.set_style(Style::Clean);
    let b = e.add_band(Shape::HighShelf, 9000.0, 3.0, 0.7).unwrap();
    e.set(Pid::b(b, B::Dynamic), 1.0);
    e.set(Pid::c(Style::Clean, C::Phase), 1.0);
    e.set(Pid::k(Style::Maag, 1), 4.0);
    e.set(Pid::g(G::Link), 0.0);
    e.select_ab(1);
    let data = e.save_state();
    let f = Engine::new();
    assert!(f.load_state(&data));
    assert_eq!(f.style(), Style::Clean);
    assert_eq!(f.params().get(Pid::b(b, B::Freq)), 9000.0);
    assert!(f.params().on(Pid::b(b, B::Dynamic)));
    assert_eq!(f.params().c(Style::Clean, C::Phase), 1.0);
    assert_eq!(f.params().get(Pid::k(Style::Maag, 1)), 4.0);
    assert!(!f.params().global().link);
    assert_eq!(f.ab(), 1);
    assert!(!f.load_state(b""));
    assert!(!f.load_state(b"garbage"));
}

#[test]
fn latency_follows_the_style_and_phase_mode() {
    let e = Arc::new(Engine::new());
    let mut p = e.processor(SR, 512);
    let mut buf = vec![0.0f32; 512];
    e.set_style(Style::Clean);
    p.process(&mut [&mut buf]);
    assert_eq!(p.latency(), 0);
    e.take_latency_changed();
    e.set(Pid::c(Style::Clean, C::Phase), 1.0);
    p.process(&mut [&mut buf]);
    assert!(p.latency() > 1000, "{}", p.latency());
    assert!(e.take_latency_changed());
    e.set_style(Style::Pultec);
    p.process(&mut [&mut buf]);
    assert!((1..100).contains(&p.latency()), "{}", p.latency());
    for s in Style::ALL {
        println!("latency {}: {:.2} ms", s.name(), e.latency_ms(s));
    }
}

#[test]
fn bypass_is_bit_identical_after_the_latency() {
    let e = Arc::new(Engine::new());
    e.load_preset(PRESETS.iter().position(|p| p.name == "Vocals").unwrap());
    e.set(Pid::g(G::Bypass), 1.0);
    let x = music(1.0);
    let y = play(&e, &x);
    let d = e.latency() as usize;
    // Allow the bypass fade at the start.
    let start = 4800;
    for i in start..x.len() {
        assert_eq!(y[i].to_bits(), x[i - d].to_bits(), "sample {i}");
    }
}

#[test]
fn display_feeds_flow() {
    let e = Arc::new(Engine::new());
    play(&e, &music(1.0));
    let mut n = 0;
    e.drain_columns(|c| {
        n += 1;
        assert!(c.before.0 <= c.before.1 && c.after.0 <= c.after.1);
    });
    let per_s = e.columns_per_second();
    assert!((n as f32 - per_s).abs() <= 2.0, "{n} vs {per_s}");
    let (mut b, mut a) = (0, 0);
    e.drain_spectrum(|_| b += 1, |_| a += 1);
    assert!(b > 4000 && a == b, "{b} {a}");
}
