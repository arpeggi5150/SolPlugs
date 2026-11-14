//! The brief's acceptance tests that the DSP alone can check. (MATCH, per-version memory,
//! auto-makeup, latency through the engine and the UI ones live in the app crate's tests.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solplugs_delay_core::line::Svf;
use solplugs_delay_core::util::lin_to_db;
use solplugs_delay_core::{
    DelayFx, Extra, Global, NOTES, Settings, TapTempo, Version, VersionSettings, latency_for,
    note_ms,
};

const SR: f32 = 48_000.0;

fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            amp * ((s >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
        })
        .collect()
}

fn sine(f: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin())
        .collect()
}

/// The singing voice from the pitch tests: a real source.
fn voice() -> Vec<f32> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pitch-core/testdata/singing-female.wav"
    );
    let mut r = hound::WavReader::open(path).expect("test wav");
    let spec = r.spec();
    let ch = spec.channels as usize;
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap() as f32 / full)
                .collect()
        }
    };
    let mono: Vec<f32> = data.chunks(ch).map(|c| c[0]).collect();
    // Resample by nearest neighbour to 48 k if needed (a level/colour test, not pitch).
    let ratio = spec.sample_rate as f32 / SR;
    let n = (mono.len() as f32 / ratio) as usize;
    (0..n.min((SR * 4.0) as usize))
        .map(|i| mono[((i as f32 * ratio) as usize).min(mono.len() - 1)])
        .collect()
}

fn global(v: Version) -> Global {
    Global {
        version: v,
        ..Global::default()
    }
}

/// `v` all wet (mix 100 %), modulation off, the given feedback, unsynced time.
fn wet(v: Version, time_ms: f32, fb: f32) -> Settings {
    let mut s = Settings::default();
    let vs = &mut s.versions[v.index()];
    vs.mix = 1.0;
    vs.sync = false;
    vs.time_ms = time_ms;
    vs.feedback = fb;
    vs.mod_depth = 0.0;
    s
}

fn vs(s: &mut Settings, v: Version) -> &mut VersionSettings {
    &mut s.versions[v.index()]
}

fn run_sc(
    fx: &mut DelayFx,
    l: &[f32],
    r: &[f32],
    sc: Option<(&[f32], &[f32])>,
    g: &Global,
    s: &Settings,
) -> (Vec<f32>, Vec<f32>) {
    let mut a = l.to_vec();
    let mut b = r.to_vec();
    let mut pos = 0;
    for (ca, cb) in a.chunks_mut(441).zip(b.chunks_mut(441)) {
        let n = ca.len();
        let key = sc.map(|(x, y)| [&x[pos..pos + n], &y[pos..pos + n]]);
        fx.process(&mut [ca, cb], key, g, s);
        pos += n;
    }
    (a, b)
}

fn run(v: Version, l: &[f32], r: &[f32], s: &Settings) -> (Vec<f32>, Vec<f32>) {
    let mut fx = DelayFx::new(SR);
    run_sc(&mut fx, l, r, None, &global(v), s)
}

fn impulse(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 0.5;
    x
}

fn energy(x: &[f32]) -> f32 {
    x.iter().map(|v| v * v).sum::<f32>()
}

fn rms_db(x: &[f32]) -> f32 {
    lin_to_db((energy(x) / x.len().max(1) as f32).sqrt())
}

/// Energy of `x` above `hz` (8th-order high-pass) over its total energy.
fn high_share(x: &[f32], hz: f32) -> f32 {
    let mut f = [Svf::default(); 4];
    for f in &mut f {
        f.set(hz, SR);
    }
    let hp: Vec<f32> = x
        .iter()
        .map(|&v| f.iter_mut().fold(v, |v, f| f.hp(v)))
        .collect();
    energy(&hp) / energy(x).max(1e-20)
}

/// Energy of `x` below `hz` (8th-order low-pass) over its total energy.
fn low_share(x: &[f32], hz: f32) -> f32 {
    let mut f = [Svf::default(); 4];
    for f in &mut f {
        f.set(hz, SR);
    }
    let lp: Vec<f32> = x
        .iter()
        .map(|&v| f.iter_mut().fold(v, |v, f| f.lp(v)))
        .collect();
    energy(&lp) / energy(x).max(1e-20)
}

/// Amplitude of the `f` Hz component of `x`.
fn amp_at(x: &[f32], f: f32) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate() {
        let p = std::f64::consts::TAU * f as f64 * i as f64 / SR as f64;
        re += *v as f64 * p.cos();
        im += *v as f64 * p.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn ms(t: f32) -> usize {
    (t * 0.001 * SR).round() as usize
}

/// The window of `x` around repeat `k` of a delay of `d_ms` (`w_ms` wide).
fn around(x: &[f32], k: usize, d_ms: f32, w_ms: f32) -> &[f32] {
    let c = ms(d_ms * k as f32);
    let h = ms(w_ms) / 2;
    &x[c.saturating_sub(h)..(c + h).min(x.len())]
}

/// Test 1: Each version sounds like itself on a real voice: Clean stays clear, Tape darkens
/// and Analog darkens most; the three differ clearly.
#[test]
fn every_version_sounds_like_itself() {
    let x = voice();
    let mut tail = vec![0.0; ms(3000.0)];
    let mut src = x.clone();
    src.append(&mut tail);
    let mut outs = Vec::new();
    for v in Version::ALL {
        let mut s = wet(v, 300.0, 0.6);
        vs(&mut s, v).mod_depth = 0.3;
        let (l, _) = run(v, &src, &src, &s);
        assert!(l.iter().all(|y| y.is_finite()));
        // The repeats' brightness, after the voice stops.
        let after = &l[x.len()..];
        let b = high_share(after, 1200.0);
        println!(
            "{v:?}: share above 1.2 kHz {b:.4}, level {:.1} dB",
            rms_db(after)
        );
        let lvl = rms_db(after);
        outs.push((l, b, lvl));
    }
    let (b_clean, b_tape) = (outs[0].1, outs[1].1);
    assert!(b_tape < b_clean * 0.7, "Tape darker than Clean");
    assert!(
        outs[2].2 < outs[1].2 - 2.0,
        "Analog melts away faster than Tape"
    );
    assert!(outs[0].2 > outs[1].2, "Clean keeps the most level");
    // On a broadband hit, by the third repeat Analog is the darkest.
    let mut hit = noise(ms(20.0), 0.4, 77);
    hit.resize(ms(1200.0), 0.0);
    let third: Vec<f32> = Version::ALL
        .iter()
        .map(|&v| {
            let (l, _) = run(v, &hit, &hit, &wet(v, 300.0, 0.6));
            high_share(around(&l, 3, 300.0, 80.0), 2000.0)
        })
        .collect();
    println!("third repeat, share above 2 kHz: {third:?}");
    assert!(third[2] < third[1] && third[1] < third[0], "{third:?}");
    for i in 0..3 {
        for j in i + 1..3 {
            let d: f32 = outs[i]
                .0
                .iter()
                .zip(&outs[j].0)
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            assert!(d / energy(&outs[i].0) > 0.05, "{i} vs {j} too alike");
        }
    }
}

/// Test 2: Tempo-sync: every note value lands exactly where the tempo says, and the 8th repeat
/// is still exactly 8 times as late (no drift).
#[test]
fn tempo_sync_locks_with_no_drift() {
    for bpm in [120.0, 97.0, 174.0] {
        for (note, name) in NOTES.iter().enumerate() {
            let d = note_ms(note, bpm);
            if d * 8.0 > 16_000.0 {
                continue;
            }
            let mut s = wet(Version::Clean, 100.0, 0.9);
            let c = vs(&mut s, Version::Clean);
            c.sync = true;
            c.note = note;
            let g = Global {
                bpm,
                ..global(Version::Clean)
            };
            let n = ms(d * 8.0 + 200.0);
            let x = impulse(n);
            let mut fx = DelayFx::new(SR);
            let (l, _) = run_sc(&mut fx, &x, &x, None, &g, &s);
            for k in [1usize, 8] {
                let want = d * 0.001 * SR * k as f32;
                let lo = (want as usize).saturating_sub(40);
                let seg = &l[lo..(lo + 80).min(l.len())];
                let (i, _) = seg.iter().enumerate().fold((0, 0.0f32), |a, (i, v)| {
                    if v.abs() > a.1 { (i, v.abs()) } else { a }
                });
                // The peak of the interpolated impulse sits within half a sample.
                let got = (lo + i) as f32;
                assert!(
                    (got - want).abs() <= 1.0,
                    "{bpm} BPM {name} repeat {k}: {got} vs {want}"
                );
            }
        }
    }
}

/// Test 3: Tap tempo: the time follows the taps.
#[test]
fn tap_tempo_follows_the_taps() {
    let mut t = TapTempo::default();
    assert_eq!(t.tap(10.0), None);
    let ms_ = t.tap(10.5).unwrap();
    assert!((ms_ - 500.0).abs() < 0.5);
    t.tap(11.0);
    let ms_ = t.tap(11.5).unwrap();
    assert!((ms_ - 500.0).abs() < 0.5);
    // A faster pair moves the average toward it.
    let ms_ = t.tap(11.8).unwrap();
    assert!(ms_ < 500.0 && ms_ > 300.0, "{ms_}");
    // A long pause starts over.
    assert_eq!(t.tap(20.0), None);
    let ms_ = t.tap(20.25).unwrap();
    assert!((ms_ - 250.0).abs() < 0.5);
}

/// Test 4: High feedback runs away into self-oscillation, and settles there: bounded, steady,
/// no digital scream.
#[test]
fn feedback_self_oscillates_cleanly() {
    for v in Version::ALL {
        let s = wet(v, 250.0, 1.15);
        let n = ms(12_000.0);
        let mut x = noise(ms(100.0), 0.3, 3);
        x.resize(n, 0.0);
        let (l, r) = run(v, &x, &x, &s);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, y| m.max(y.abs()));
        let a = rms_db(&l[ms(10_000.0)..ms(11_000.0)]);
        let b = rms_db(&l[ms(11_000.0)..]);
        println!("{v:?}: peak {peak:.3}, last seconds {a:.1} / {b:.1} dB");
        assert!(l.iter().all(|y| y.is_finite()));
        assert!(peak <= 1.05, "{v:?} peak {peak}");
        assert!(b > -30.0, "{v:?} still ringing: {b}");
        assert!((a - b).abs() < 3.0, "{v:?} steady: {a} vs {b}");
    }
}

/// Test 5: The low-pass in the loop darkens the repeats each time round.
#[test]
fn loop_filter_darkens_each_repeat() {
    let v = Version::Clean;
    let mut s = wet(v, 200.0, 0.8);
    vs(&mut s, v).filter_lp = 2500.0;
    let mut x = noise(ms(20.0), 0.4, 9);
    x.resize(ms(1200.0), 0.0);
    let (l, _) = run(v, &x, &x, &s);
    let shares: Vec<f32> = (1..=5)
        .map(|k| high_share(around(&l, k, 200.0, 60.0), 4000.0))
        .collect();
    println!("share above 4 kHz per repeat: {shares:?}");
    for w in shares.windows(2) {
        assert!(w[1] < w[0] * 0.8, "{shares:?}");
    }
}

/// Test 6: Ping-pong: the repeats bounce left, right, left.
#[test]
fn ping_pong_bounces() {
    let v = Version::Clean;
    let mut s = wet(v, 150.0, 0.7);
    vs(&mut s, v).stereo_mix = 0.0;
    vs(&mut s, v).pingpong_mix = 1.0;
    let x = impulse(ms(700.0));
    let (l, r) = run(v, &x, &x, &s);
    for k in 1..=4 {
        let el = energy(around(&l, k, 150.0, 20.0));
        let er = energy(around(&r, k, 150.0, 20.0));
        let (hot, cold) = if k % 2 == 1 { (el, er) } else { (er, el) };
        assert!(hot > 1e-4 && cold < hot * 1e-4, "repeat {k}: L {el} R {er}");
    }
}

/// Spread of the zero-crossing intervals of `x` (samples): 0 for a steady tone.
fn period_spread(x: &[f32]) -> f32 {
    let mut zc = Vec::new();
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let f = x[i - 1] / (x[i - 1] - x[i]);
            zc.push(i as f32 - 1.0 + f);
        }
    }
    let p: Vec<f32> = zc.windows(2).map(|w| w[1] - w[0]).collect();
    let lo = p.iter().cloned().fold(f32::MAX, f32::min);
    let hi = p.iter().cloned().fold(0.0, f32::max);
    hi - lo
}

/// Test 7: Modulation: Tape wobbles, Analog choruses, Clean stays still (even at 100 %).
#[test]
fn modulation_per_version() {
    let x = sine(1000.0, 0.3, ms(4000.0));
    let mut spread = Vec::new();
    for v in Version::ALL {
        let mut s = wet(v, 120.0, 0.0);
        vs(&mut s, v).mod_depth = 1.0;
        let (l, _) = run(v, &x, &x, &s);
        let sp = period_spread(&l[ms(1000.0)..]);
        println!("{v:?}: period spread {sp:.3} samples");
        spread.push(sp);
    }
    assert!(spread[0] < 0.05, "Clean still: {}", spread[0]);
    assert!(spread[1] > 0.2, "Tape wobbles: {}", spread[1]);
    assert!(spread[2] > 0.2, "Analog choruses: {}", spread[2]);
}

/// Test 9: Bypass is bit-identical to the input, on every version.
#[test]
fn bypass_is_bit_identical() {
    for v in Version::ALL {
        let x = noise(ms(500.0), 0.5, 11);
        let y = noise(ms(500.0), 0.5, 12);
        let g = Global {
            bypass: true,
            ..global(v)
        };
        let mut fx = DelayFx::new(SR);
        let (l, r) = run_sc(&mut fx, &x, &y, None, &g, &Settings::default());
        assert_eq!(l, x);
        assert_eq!(r, y);
    }
}

/// Test 10: 50 % mix: a parallel delay, the dry half of the output is the input itself.
#[test]
fn half_mix_keeps_the_dry_intact() {
    for v in Version::ALL {
        let x = noise(ms(800.0), 0.3, 5);
        let mut s = Settings::default();
        vs(&mut s, v).mix = 0.5;
        let (l, _) = run(v, &x, &x, &s);
        vs(&mut s, v).dry_db = -60.0;
        let (w, _) = run(v, &x, &x, &s);
        let err = l
            .iter()
            .zip(&w)
            .zip(&x)
            .fold(0.0f32, |m, ((a, b), d)| m.max((a - b - 0.5 * d).abs()));
        assert!(err < 1e-6, "{v:?}: {err}");
    }
}

/// Test 13: Dual mono treats each side on its own; linked spreads the repeats across.
#[test]
fn link_vs_dual_mono() {
    for v in Version::ALL {
        let mut s = wet(v, 120.0, 0.6);
        vs(&mut s, v).mod_depth = 0.3;
        if let Some(i) = v.extra(Extra::Noise) {
            // The chip's hiss is on both sides by nature; this checks the signal path.
            vs(&mut s, v).extras[i] = 0.0;
        }
        let x = impulse(ms(800.0));
        let z = vec![0.0; x.len()];
        let mut fx = DelayFx::new(SR);
        let g = Global {
            link: false,
            ..global(v)
        };
        let (_, r) = run_sc(&mut fx, &x, &z, None, &g, &s);
        assert!(energy(&r) < 1e-12, "{v:?} dual mono leaked {}", energy(&r));
        let mut fx = DelayFx::new(SR);
        let (l, r) = run_sc(&mut fx, &x, &z, None, &global(v), &s);
        assert!(energy(&r) > energy(&l) * 1e-3, "{v:?} linked spreads");
    }
}

/// Test 15: Latency: zero on every version, and the dry path really is undelayed.
#[test]
fn latency_is_zero_and_true() {
    for v in Version::ALL {
        assert_eq!(latency_for(v, &Settings::default(), SR), 0);
        let mut s = Settings::default();
        vs(&mut s, v).mix = 0.0;
        let x = noise(ms(100.0), 0.5, 2);
        let (l, _) = run(v, &x, &x, &s);
        let err = l
            .iter()
            .zip(&x)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(err < 1e-6, "{v:?}: {err}");
    }
}

/// Test 16: The stereo delay and the ping-pong delay each have their own wet level; together
/// they add up, and the dry stays intact.
#[test]
fn stereo_and_ping_pong_blend_on_their_own() {
    let v = Version::Clean;
    let x = noise(ms(30.0), 0.3, 21);
    let mut x2 = x.clone();
    x2.resize(ms(900.0), 0.0);
    let render = |st: f32, pp: f32| {
        let mut s = wet(v, 160.0, 0.5);
        vs(&mut s, v).stereo_mix = st;
        vs(&mut s, v).pingpong_mix = pp;
        vs(&mut s, v).offset_ms = 7.0;
        run(v, &x2, &x2, &s)
    };
    let (al, ar) = render(1.0, 0.0);
    let (bl, br) = render(0.0, 1.0);
    let (cl, cr) = render(1.0, 1.0);
    let (hl, _) = render(0.5, 0.0);
    let mut err = 0.0f32;
    for i in 0..cl.len() {
        err = err.max((cl[i] - al[i] - bl[i]).abs());
        err = err.max((cr[i] - ar[i] - br[i]).abs());
        err = err.max((hl[i] - 0.5 * al[i]).abs());
    }
    assert!(err < 1e-4, "{err}");
    assert!(energy(&bl) > 0.0 && energy(&al) > 0.0);
    // Dry intact while both blend: 50 % mix minus its wet part is half the input.
    let mut s = wet(v, 160.0, 0.5);
    vs(&mut s, v).pingpong_mix = 0.6;
    vs(&mut s, v).stereo_mix = 0.3;
    vs(&mut s, v).mix = 0.5;
    let (m, _) = run(v, &x2, &x2, &s);
    vs(&mut s, v).dry_db = -60.0;
    let (w, _) = run(v, &x2, &x2, &s);
    let e = m
        .iter()
        .zip(&w)
        .zip(&x2)
        .fold(0.0f32, |a, ((m, w), d)| a.max((m - w - 0.5 * d).abs()));
    assert!(e < 1e-6, "{e}");
}

/// Test 19: The Low Cut and High Cut knobs roll the repeats off; the dry is untouched.
#[test]
fn low_and_high_cut_roll_the_repeats_off() {
    let v = Version::Clean;
    let x = noise(ms(1500.0), 0.3, 31);
    let mut s = wet(v, 100.0, 0.0);
    let (open, _) = run(v, &x, &x, &s);
    vs(&mut s, v).low_cut_hz = 600.0;
    vs(&mut s, v).high_cut_hz = 3000.0;
    let (cut, _) = run(v, &x, &x, &s);
    let tail = |y: &[f32]| y[ms(300.0)..].to_vec();
    let lows = |y: &[f32]| low_share(y, 150.0);
    let (o, c) = (tail(&open), tail(&cut));
    println!(
        "lows {:.3} -> {:.3}, highs {:.3} -> {:.3}",
        lows(&o),
        lows(&c),
        high_share(&o, 8000.0),
        high_share(&c, 8000.0)
    );
    assert!(lows(&c) < lows(&o) * 0.25);
    assert!(high_share(&c, 8000.0) < high_share(&o, 8000.0) * 0.1);
    // No clicks when they move: the output stays bounded and finite.
    assert!(c.iter().all(|y| y.is_finite() && y.abs() < 1.0));
}

/// Test 20: LoFi: off by default; on, it adds grit (energy away from the tone).
#[test]
fn lofi_button_adds_grit() {
    for v in Version::ALL {
        assert!(!VersionSettings::defaults(v).lofi);
    }
    let v = Version::Clean;
    let x = sine(3100.0, 0.3, ms(1000.0));
    let mut s = wet(v, 50.0, 0.0);
    let (clean, _) = run(v, &x, &x, &s);
    vs(&mut s, v).lofi = true;
    let (grit, _) = run(v, &x, &x, &s);
    let junk = |y: &[f32]| {
        let y = &y[ms(200.0)..];
        let a = amp_at(y, 3100.0);
        let tone = a * a / 2.0 * y.len() as f32;
        (energy(y) - tone).max(0.0) / energy(y)
    };
    println!("grit {:.5} -> {:.5}", junk(&clean), junk(&grit));
    assert!(junk(&clean) < 1e-3);
    assert!(junk(&grit) > 0.01);
}

/// Test 21: Analog button: off by default; on, it adds warmth (even harmonics) to the repeats.
#[test]
fn analog_button_adds_warmth() {
    for v in Version::ALL {
        assert!(!VersionSettings::defaults(v).analog);
    }
    let v = Version::Clean;
    let x = sine(500.0, 0.5, ms(1000.0));
    let mut s = wet(v, 50.0, 0.0);
    let (clean, _) = run(v, &x, &x, &s);
    vs(&mut s, v).analog = true;
    let (warm, _) = run(v, &x, &x, &s);
    let h2 = |y: &[f32]| {
        let y = &y[ms(200.0)..];
        amp_at(y, 1000.0) / amp_at(y, 500.0)
    };
    println!("2nd harmonic {:.5} -> {:.5}", h2(&clean), h2(&warm));
    assert!(h2(&clean) < 1e-3);
    assert!(h2(&warm) > 0.02);
}

/// Test 22: Ducking: the repeats duck while the dry plays and come back up when it stops; an
/// external sidechain can drive it instead.
#[test]
fn ducking_follows_the_dry_and_the_sidechain() {
    let v = Version::Clean;
    let mut x = noise(ms(1000.0), 0.3, 41);
    x.resize(ms(2500.0), 0.0);
    let mut s = wet(v, 250.0, 0.85);
    let (plain, _) = run(v, &x, &x, &s);
    vs(&mut s, v).duck = 1.0;
    vs(&mut s, v).duck_release_ms = 150.0;
    let (ducked, _) = run(v, &x, &x, &s);
    let during = |y: &[f32]| rms_db(&y[ms(500.0)..ms(1000.0)]);
    let after = |y: &[f32]| rms_db(&y[ms(1700.0)..ms(2200.0)]);
    println!(
        "during {:.1} -> {:.1} dB, after {:.1} -> {:.1} dB",
        during(&plain),
        during(&ducked),
        after(&plain),
        after(&ducked)
    );
    assert!(
        during(&ducked) < during(&plain) - 15.0,
        "ducks while the dry plays"
    );
    assert!(after(&ducked) > after(&plain) - 1.5, "comes back up after");

    // External sidechain: the dry is silent in the key, a burst on inputs 3/4 ducks.
    vs(&mut s, v).duck_ext = true;
    let sig = noise(ms(2500.0), 0.2, 42);
    let mut key = vec![0.0; ms(2500.0)];
    for k in key.iter_mut().take(ms(2000.0)).skip(ms(1000.0)) {
        *k = 0.5;
    }
    let mut fx = DelayFx::new(SR);
    let (y, _) = run_sc(&mut fx, &sig, &sig, Some((&key, &key)), &global(v), &s);
    let open = rms_db(&y[ms(500.0)..ms(1000.0)]);
    let shut = rms_db(&y[ms(1300.0)..ms(1900.0)]);
    println!("sidechain: {open:.1} -> {shut:.1} dB");
    assert!(shut < open - 15.0);
}

/// Test 23: Reverse: the repeats play backwards (a sharp hit with a slow tail comes back as a
/// slow swell that ends sharp).
#[test]
fn reverse_plays_backwards() {
    let v = Version::Clean;
    let n = ms(1600.0);
    // A plucked tone: sharp attack, 60 ms decay, then silence.
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            if t < 0.25 {
                0.5 * (-t / 0.06).exp() * (std::f32::consts::TAU * 330.0 * t).sin()
            } else {
                0.0
            }
        })
        .collect();
    let mut s = wet(v, 500.0, 0.0);
    let env = |y: &[f32]| -> Vec<f32> {
        y.chunks(ms(10.0))
            .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
            .collect()
    };
    // Where, within the echo, the loudest moment sits relative to its energy centre.
    let shape = |y: &[f32]| {
        let e = env(y);
        let tot: f32 = e.iter().map(|v| v * v).sum();
        let centre: f32 = e
            .iter()
            .enumerate()
            .map(|(i, v)| i as f32 * v * v)
            .sum::<f32>()
            / tot;
        let (imax, _) = e
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |a, (i, v)| if *v > a.1 { (i, *v) } else { a });
        imax as f32 - centre
    };
    let (fwd, _) = run(v, &x, &x, &s);
    vs(&mut s, v).reverse = true;
    let (rev, _) = run(v, &x, &x, &s);
    let a = shape(&fwd);
    let b = shape(&rev);
    println!("peak minus centre: forward {a:.1}, reverse {b:.1} (10 ms steps)");
    assert!(a < 0.0, "forward: peak first");
    assert!(b > 0.0, "reverse: peak last");
    assert!(energy(&rev) > energy(&fwd) * 0.2);
}

/// Test 24: L/R offset: the right side lands later than the left.
#[test]
fn offset_lands_the_right_side_later() {
    let v = Version::Clean;
    let mut s = wet(v, 200.0, 0.0);
    vs(&mut s, v).offset_ms = 15.0;
    let x = impulse(ms(400.0));
    let (l, r) = run(v, &x, &x, &s);
    let peak_at = |y: &[f32]| {
        y.iter()
            .enumerate()
            .fold(
                (0, 0.0f32),
                |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a },
            )
            .0
    };
    assert_eq!(peak_at(&l), ms(200.0));
    assert_eq!(peak_at(&r), ms(215.0));
}

/// No clicks when the time changes, on every version: jump the time while a tone plays and
/// the output never jumps.
#[test]
fn no_clicks_when_the_time_changes() {
    for v in Version::ALL {
        let mut x = sine(440.0, 0.1, ms(4000.0));
        // Fade the tone in, so its own start isn't a click.
        for (i, v) in x.iter_mut().take(ms(20.0)).enumerate() {
            *v *= i as f32 / ms(20.0) as f32;
        }
        let mut fx = DelayFx::new(SR);
        let g = global(v);
        let mut s = wet(v, 300.0, 0.4);
        let mut out = Vec::new();
        for (i, block) in x.chunks(480).enumerate() {
            vs(&mut s, v).time_ms = match i / 50 {
                0 | 1 => 300.0,
                2 => 310.0,
                3 => 520.0,
                4 => 90.0,
                _ => 1300.0,
            };
            let mut a = block.to_vec();
            let mut b = block.to_vec();
            fx.process(&mut [&mut a, &mut b], None, &g, &s);
            out.extend(a);
        }
        // A click is a lone jump in the second difference, far above what the signal around
        // it does (a tone, even one whose pitch bends as the tape glides, is smooth).
        let d2: Vec<f32> = out.windows(3).map(|w| w[2] - 2.0 * w[1] + w[0]).collect();
        let mut pre = vec![0.0f64; d2.len() + 1];
        for (i, v) in d2.iter().enumerate() {
            pre[i + 1] = pre[i] + f64::from(v * v);
        }
        let h = 96;
        let mut worst = 0.0f32;
        let mut at = 0;
        for i in ms(400.0)..d2.len() - h {
            let rms = ((pre[i + h] - pre[i - h]) / (2 * h) as f64).sqrt() as f32;
            let k = d2[i].abs() / (rms + 1e-5);
            if k > worst {
                worst = k;
                at = i;
            }
        }
        println!(
            "{v:?}: worst spike {worst:.2}x local at {:.0} ms",
            at as f32 / 48.0
        );
        assert!(worst < 4.0, "{v:?}: {worst}");
    }
}

/// Tape's extra heads add earlier echoes.
#[test]
fn tape_heads_add_echoes() {
    let v = Version::Tape;
    let mut s = wet(v, 400.0, 0.0);
    let x = impulse(ms(600.0));
    let (one, _) = run(v, &x, &x, &s);
    let i = v.extra(Extra::Heads).unwrap();
    vs(&mut s, v).extras[i] = 2.0;
    let (three, _) = run(v, &x, &x, &s);
    let early = |y: &[f32]| energy(&y[ms(150.0)..ms(350.0)]);
    assert!(early(&one) < 1e-8);
    assert!(early(&three) > 1e-4);
}
