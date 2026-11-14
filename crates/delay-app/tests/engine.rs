//! Engine behaviour: per-version memory, A/B, presets, MATCH, auto-makeup, Gain In/Out,
//! tap tempo, host tempo, the sidechain, state, latency, bypass and the display feeds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use solplugs_delay_app::engine::{Engine, HostEvent};
use solplugs_delay_app::params::{G, Pid};
use solplugs_delay_app::presets::PRESETS;
use solplugs_delay_core::note_ms;
use solplugs_delay_core::{Ctl, Version};

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

fn play(p: &mut solplugs_delay_app::engine::Processor, x: &[f32]) -> Vec<f32> {
    let mut l = x.to_vec();
    let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
        p.process(&mut [a, b], None, None);
    }
    l
}

/// Test 12: per-version memory.
#[test]
fn every_version_remembers_its_own_settings() {
    let e = Engine::new();
    e.set_version(Version::Tape);
    e.set(Pid::c(Version::Tape, Ctl::Feedback), 77.0);
    e.set(Pid::x(Version::Tape, 1), 80.0);
    e.set_version(Version::Analog);
    e.set(Pid::c(Version::Analog, Ctl::Feedback), 12.0);
    e.set_version(Version::Tape);
    let p = e.params();
    assert_eq!(p.c(Version::Tape, Ctl::Feedback), 77.0);
    assert_eq!(p.get(Pid::x(Version::Tape, 1)), 80.0);
    assert!((p.c(Version::Analog, Ctl::Feedback) - 12.0).abs() < 1e-6);
    // Another version is untouched.
    assert!(
        (p.c(Version::Clean, Ctl::Feedback) - f64::from(Version::Clean.default_of(Ctl::Feedback)))
            .abs()
            < 1e-6
    );
}

#[test]
fn ui_edits_reach_the_host() {
    let e = Engine::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    e.set_listener(Some(Arc::new(move |ev| s.lock().unwrap().push(ev))));
    let id = Pid::c(Version::Clean, Ctl::Width);
    e.set(id, 70.0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(matches!(seen[1], HostEvent::Value(p, v) if p == id && v == 70.0));
}

#[test]
fn ab_switches_whole_setups() {
    let e = Engine::new();
    e.set_version(Version::Clean);
    e.set(Pid::c(Version::Clean, Ctl::Mix), 40.0);
    e.select_ab(1);
    assert_eq!(e.version(), Version::Clean);
    e.set_version(Version::Analog);
    e.set(Pid::c(Version::Analog, Ctl::Mix), 60.0);
    e.select_ab(0);
    assert_eq!(e.version(), Version::Clean);
    e.select_ab(1);
    assert_eq!(e.version(), Version::Analog);
    assert_eq!(e.params().c(Version::Analog, Ctl::Mix), 60.0);
}

#[test]
fn presets_load_and_cover_the_brief() {
    for name in [
        "Slapback",
        "Dotted Eighth",
        "Ping-Pong",
        "Tape Echo",
        "Dub",
        "Vocal Echo",
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
    e.set_version(Version::Analog);
    e.set(Pid::c(Version::Analog, Ctl::Time), 333.0);
    e.set(Pid::g(G::Tempo), 98.0);
    e.set_stacked(true);
    e.select_ab(1);
    let data = e.save_state();
    let f = Engine::new();
    assert!(f.load_state(&data));
    assert_eq!(f.version(), Version::Analog);
    assert!((f.params().c(Version::Analog, Ctl::Time) - 333.0).abs() < 1e-3);
    assert_eq!(f.params().get(Pid::g(G::Tempo)), 98.0);
    assert!(f.stacked());
    assert_eq!(f.ab(), 1);
    assert!(!f.load_state(b"not a state"));
}

/// Test 8: MATCH: the after average peak lands within 0.5 dB of the before.
#[test]
fn match_lands_within_half_a_db() {
    for v in Version::ALL {
        let e = Arc::new(Engine::new());
        e.set_version(v);
        e.set(Pid::c(v, Ctl::Mix), 60.0);
        e.set(Pid::c(v, Ctl::Wet), 6.0);
        e.set(Pid::c(v, Ctl::Feedback), 50.0);
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

/// Test 14: auto-makeup holds the output level when the wet level changes.
#[test]
fn auto_makeup_holds_the_level() {
    let v = Version::Tape;
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

/// Gain In and Gain Out work on every version; Gain Out is the makeup.
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

/// Tests 9 and 15: bypass is bit-identical through the engine, and latency is reported (zero).
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

/// Test 3: tap tempo through the engine: the time follows the taps and sync turns off.
#[test]
fn tap_tempo_sets_the_time() {
    let e = Engine::new();
    let v = e.version();
    assert!(e.params().c(v, Ctl::Sync) >= 0.5);
    assert_eq!(e.tap_at(1.0), None);
    let ms = e.tap_at(1.4).unwrap();
    assert!((ms - 400.0).abs() < 0.5);
    e.tap_at(1.8);
    assert!((e.params().c(v, Ctl::Time) - 400.0).abs() < 0.5);
    assert!(e.params().c(v, Ctl::Sync) < 0.5);
}

/// Test 2: tempo-sync follows the host's tempo when it sends one, else the Tempo knob.
#[test]
fn tempo_follows_the_host() {
    let e = Arc::new(Engine::new());
    let v = e.version();
    e.set(Pid::c(v, Ctl::Note), 7.0); // 1/4
    let mut p = e.processor(SR, 512);
    let mut a = vec![0.0f32; 512];
    let mut b = vec![0.0f32; 512];
    p.process(&mut [&mut a, &mut b], None, Some(90.0));
    assert!(e.meters().host_tempo());
    assert!((e.meters().delay_ms() - note_ms(7, 90.0)).abs() < 1e-3);
    p.process(&mut [&mut a, &mut b], None, None);
    assert!(!e.meters().host_tempo());
    assert!((e.meters().delay_ms() - 500.0).abs() < 1e-3);
}

/// Test 22 (external key): the sidechain on inputs 3/4 reaches the ducker.
#[test]
fn sidechain_reaches_the_ducker() {
    let e = Arc::new(Engine::new());
    let v = Version::Clean;
    e.set_version(v);
    e.set(Pid::c(v, Ctl::Duck), 100.0);
    e.set(Pid::c(v, Ctl::DuckSource), 1.0);
    let mut p = e.processor(SR, 512);
    let x = phrase(1.0);
    let key = vec![0.5f32; x.len()];
    let mut l = x.clone();
    let mut r = x.clone();
    for ((a, b), k) in l
        .chunks_mut(480)
        .zip(r.chunks_mut(480))
        .zip(key.chunks(480))
    {
        p.process(&mut [a, b], Some([k, k]), None);
    }
    assert!(e.meters().duck() > 20.0, "{}", e.meters().duck());
}

/// Test 18: the meters keep left and right apart.
#[test]
fn meters_show_left_and_right() {
    let e = Arc::new(Engine::new());
    let mut p = e.processor(SR, 512);
    let x = phrase(1.0);
    let mut l = x.clone();
    let mut r = vec![0.0f32; x.len()];
    e.set(Pid::g(G::Link), 0.0);
    e.set(Pid::c(e.version(), Ctl::Mix), 0.0);
    for (a, b) in l.chunks_mut(480).zip(r.chunks_mut(480)) {
        p.process(&mut [a, b], None, None);
    }
    let (il, ir) = e.meters().input_lr();
    let (ol, or) = e.meters().output_lr();
    assert!(il > -40.0 && ir < -100.0, "{il} {ir}");
    assert!(ol > -40.0 && or < -100.0, "{ol} {or}");
    let mut n = 0;
    e.drain_columns(|c| {
        assert!(c.before[1].1 <= 0.0 && c.before[1].0 >= 0.0);
        n += 1;
    });
    assert!(n > 0);
}
