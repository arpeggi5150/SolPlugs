//! Engine behaviour: per-version memory, A/B, presets, MATCH, auto-makeup, Gain In/Out,
//! state, latency, bypass and the display feeds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use solplugs_reverb_app::engine::{Engine, HostEvent};
use solplugs_reverb_app::params::{G, Pid};
use solplugs_reverb_app::presets::PRESETS;
use solplugs_reverb_core::{Ctl, Version};

const SR: f64 = 48_000.0;

/// A vocal-like phrase: tones in syllables, with pauses.
fn phrase(seconds: f32) -> Vec<f32> {
    let n = (seconds * 48_000.0) as usize;
    let mut seed = 7u32;
    (0..n)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let w = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            let t = i as f32 / 48_000.0;
            let syll = (t * 3.1).fract();
            let env = if syll < 0.55 {
                (syll / 0.55 * std::f32::consts::PI).sin()
            } else {
                0.0
            };
            0.3 * env * ((std::f32::consts::TAU * 220.0 * t).sin() * 0.6 + 0.2 * w)
        })
        .collect()
}

fn play(p: &mut solplugs_reverb_app::engine::Processor, x: &[f32]) -> Vec<f32> {
    let mut l = x.to_vec();
    let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
        p.process(&mut [a, b]);
    }
    l
}

#[test]
fn every_version_remembers_its_own_settings() {
    let e = Engine::new();
    e.set_version(Version::Hall);
    e.set(Pid::c(Version::Hall, Ctl::Decay), 5.0);
    e.set(Pid::x(Version::Hall, 0), 80.0);
    e.set_version(Version::Plate);
    e.set(Pid::c(Version::Plate, Ctl::Decay), 0.9);
    e.set_version(Version::Hall);
    let p = e.params();
    assert_eq!(p.c(Version::Hall, Ctl::Decay), 5.0);
    assert_eq!(p.get(Pid::x(Version::Hall, 0)), 80.0);
    assert!((p.c(Version::Plate, Ctl::Decay) - 0.9).abs() < 1e-6);
    // Another version is untouched.
    assert!(
        (p.c(Version::Room, Ctl::Decay) - f64::from(Version::Room.default_of(Ctl::Decay))).abs()
            < 1e-6
    );
}

#[test]
fn ui_edits_reach_the_host() {
    let e = Engine::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    e.set_listener(Some(Arc::new(move |ev| s.lock().unwrap().push(ev))));
    let id = Pid::c(Version::Room, Ctl::Size);
    e.set(id, 70.0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(matches!(seen[1], HostEvent::Value(p, v) if p == id && v == 70.0));
}

#[test]
fn ab_switches_whole_setups() {
    let e = Engine::new();
    e.set_version(Version::Plate);
    e.set(Pid::c(Version::Plate, Ctl::Mix), 40.0);
    e.select_ab(1);
    assert_eq!(e.version(), Version::Plate);
    e.set_version(Version::Spring);
    e.set(Pid::c(Version::Spring, Ctl::Mix), 60.0);
    e.select_ab(0);
    assert_eq!(e.version(), Version::Plate);
    e.select_ab(1);
    assert_eq!(e.version(), Version::Spring);
    assert_eq!(e.params().c(Version::Spring, Ctl::Mix), 60.0);
}

#[test]
fn presets_load_and_cover_the_brief() {
    for name in [
        "Vocals",
        "Drums",
        "Guitar",
        "Mix Bus",
        "Ambient",
        "Small Room",
        "Big Hall",
    ] {
        assert!(PRESETS.iter().any(|p| p.name == name), "{name}");
    }
    for v in Version::ALL {
        assert!(
            PRESETS
                .iter()
                .any(|p| p.version == v && p.name.ends_with("Start")),
            "{v:?}"
        );
    }
    let e = Engine::new();
    for (i, p) in PRESETS.iter().enumerate() {
        e.load_preset(i);
        assert_eq!(e.version(), p.version);
        for (c, v) in p.ctl {
            assert!(
                (e.params().c(p.version, *c) - v).abs() < 1e-3,
                "{} {c:?}",
                p.name
            );
        }
    }
}

#[test]
fn state_round_trips() {
    let e = Engine::new();
    e.set_version(Version::Spring);
    e.set(Pid::c(Version::Spring, Ctl::Decay), 3.3);
    e.set_stacked(true);
    e.select_ab(1);
    let data = e.save_state();
    let f = Engine::new();
    assert!(f.load_state(&data));
    assert_eq!(f.version(), Version::Spring);
    assert!((f.params().c(Version::Spring, Ctl::Decay) - 3.3).abs() < 1e-5);
    assert!(f.stacked());
    assert_eq!(f.ab(), 1);
    assert!(!f.load_state(b"not a state"));
}

/// 7. MATCH: the after average peak lands within 0.5 dB of the before.
#[test]
fn match_lands_within_half_a_db() {
    for v in Version::ALL {
        let e = Arc::new(Engine::new());
        e.set_version(v);
        e.set(Pid::c(v, Ctl::Mix), 60.0);
        e.set(Pid::c(v, Ctl::Wet), 6.0);
        let mut p = e.processor(SR, 512);
        let x = phrase(6.0);
        play(&mut p, &x);
        play(&mut p, &x);
        let gap0 = e.meters().avg_after() - e.meters().avg_before();
        e.match_now();
        play(&mut p, &x);
        let gap = e.meters().avg_after() - e.meters().avg_before();
        println!("{v:?}: {gap0:+.2} dB -> {gap:+.2} dB after MATCH");
        assert!(gap.abs() < 0.5, "{v:?}: {gap}");
    }
}

/// 13. Auto-makeup holds the output level when the wet level changes.
#[test]
fn auto_makeup_holds_the_level() {
    let v = Version::Hall;
    let e = Arc::new(Engine::new());
    e.set_version(v);
    e.set(Pid::c(v, Ctl::Mix), 50.0);
    e.set_auto_makeup(true);
    let mut p = e.processor(SR, 512);
    let x = phrase(8.0);
    play(&mut p, &x);
    let a = e.meters().avg_after() - e.meters().avg_before();
    e.set(Pid::c(v, Ctl::Wet), 9.0);
    play(&mut p, &x);
    play(&mut p, &x);
    let b = e.meters().avg_after() - e.meters().avg_before();
    println!("auto-makeup: {a:+.2} dB, after +9 dB wet {b:+.2} dB");
    assert!(b.abs() < 1.0, "{b}");
}

/// 14. Gain In and Gain Out work on every version; Gain Out is the makeup.
#[test]
fn gain_in_and_out_on_every_version() {
    for v in Version::ALL {
        let level = |gin: f64, gout: f64| {
            let e = Arc::new(Engine::new());
            e.set_version(v);
            e.set(Pid::c(v, Ctl::Mix), 100.0);
            e.set(Pid::c(v, Ctl::GainIn), gin);
            e.set(Pid::c(v, Ctl::GainOut), gout);
            let mut p = e.processor(SR, 512);
            play(&mut p, &phrase(3.0));
            e.meters().avg_after()
        };
        let base = level(0.0, 0.0);
        let out = level(0.0, -6.0);
        let inn = level(-12.0, 0.0);
        assert!((out - base + 6.0).abs() < 0.5, "{v:?} out {base} {out}");
        assert!(inn < base - 6.0, "{v:?} in {base} {inn}");
    }
}

/// 8 / 15. Bypass is bit-identical through the engine, and latency is reported (zero).
#[test]
fn bypass_and_latency_through_the_engine() {
    for v in Version::ALL {
        let e = Arc::new(Engine::new());
        e.set_version(v);
        e.set(Pid::g(G::Bypass), 1.0);
        let mut p = e.processor(SR, 512);
        let x = phrase(1.0);
        let y = play(&mut p, &x);
        assert_eq!(y, x);
        assert_eq!(e.latency(), 0);
        assert_eq!(p.latency(), 0);
    }
}

#[test]
fn waveform_columns_flow() {
    let e = Arc::new(Engine::new());
    let mut p = e.processor(SR, 512);
    play(&mut p, &phrase(0.5));
    let mut n = 0;
    e.drain_columns(|c| {
        assert!(c.before[0].1 >= c.before[0].0);
        n += 1;
    });
    assert!(n >= 90, "{n}");
}
