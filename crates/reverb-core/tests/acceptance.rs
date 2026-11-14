//! The brief's acceptance tests that the DSP alone can check. (MATCH, per-version memory,
//! auto-makeup on the engine and the UI ones live in the app crate's tests.)

use solplugs_reverb_core::curve::{self, Band};
use solplugs_reverb_core::util::lin_to_db;
use solplugs_reverb_core::{
    Extra, Global, Reverb, Settings, Version, VersionSettings, latency_for,
};

const SR: f32 = 48_000.0;

/// Deterministic noise.
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

/// A vocal-like test source: noise bursts shaped like syllables, with pauses.
fn phrase(n: usize) -> Vec<f32> {
    let nz = noise(n, 1.0, 7);
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let syll = (t * 3.1).fract();
            let env = if syll < 0.55 {
                (syll / 0.55 * std::f32::consts::PI).sin()
            } else {
                0.0
            };
            let tone = (std::f32::consts::TAU * 220.0 * t).sin() * 0.6
                + (std::f32::consts::TAU * 440.0 * t).sin() * 0.25;
            0.3 * env * (tone + 0.25 * nz[i])
        })
        .collect()
}

fn run(rv: &mut Reverb, l: &[f32], r: &[f32], g: &Global, s: &Settings) -> (Vec<f32>, Vec<f32>) {
    let mut a = l.to_vec();
    let mut b = r.to_vec();
    for (ca, cb) in a.chunks_mut(128).zip(b.chunks_mut(128)) {
        rv.process(&mut [ca, cb], g, s);
    }
    (a, b)
}

fn global(v: Version) -> Global {
    Global {
        version: v,
        bypass: false,
        link: true,
    }
}

/// Settings with the active version all wet, dry off.
fn wet_only(v: Version) -> Settings {
    let mut s = Settings::default();
    let vs = &mut s.versions[v.index()];
    vs.mix = 1.0;
    vs.dry_db = -60.0;
    s
}

fn impulse_response(v: Version, s: &Settings, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * SR) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    l[0] = 1.0;
    r[0] = 1.0;
    let mut rv = Reverb::new(SR);
    run(&mut rv, &l, &r, &global(v), s)
}

/// Energy decay curve (Schroeder backward integral), dB, one value per 10 ms.
fn edc(y: &[f32]) -> Vec<f32> {
    let win = (SR * 0.01) as usize;
    let mut tail = 0.0f64;
    let mut out = vec![0.0f32; y.len() / win];
    let mut acc: Vec<f64> = vec![0.0; y.len() + 1];
    for i in (0..y.len()).rev() {
        tail += f64::from(y[i]) * f64::from(y[i]);
        acc[i] = tail;
    }
    let total = acc[0].max(1e-30);
    for (k, o) in out.iter_mut().enumerate() {
        *o = (10.0 * (acc[k * win] / total).max(1e-30).log10()) as f32;
    }
    out
}

/// RT60 from the -5 to -25 dB slope (T20), s.
fn t20(y: &[f32]) -> f32 {
    let e = edc(y);
    let a = e.iter().position(|v| *v < -5.0).unwrap_or(0);
    let b = e.iter().position(|v| *v < -25.0).unwrap_or(e.len() - 1);
    (b - a) as f32 * 0.01 * 3.0
}

/// Amplitude of frequency `f` in `y`, by correlation.
fn amp_at(y: &[f32], f: f32) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in y.iter().enumerate() {
        let ph = std::f64::consts::TAU * f64::from(f) * i as f64 / f64::from(SR);
        re += f64::from(*v) * ph.sin();
        im += f64::from(*v) * ph.cos();
    }
    ((re * re + im * im).sqrt() * 2.0 / y.len() as f64) as f32
}

fn mono(l: &[f32], r: &[f32]) -> Vec<f32> {
    l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect()
}

fn peak(y: &[f32]) -> f32 {
    y.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn rms(y: &[f32]) -> f32 {
    (y.iter().map(|v| v * v).sum::<f32>() / y.len().max(1) as f32).sqrt()
}

/// Level of a band of `y` (one-pole band split), dB RMS.
fn band_rms(y: &[f32], lo: f32, hi: f32) -> f32 {
    let a_hi = 1.0 - (-std::f32::consts::TAU * hi / SR).exp();
    let a_lo = 1.0 - (-std::f32::consts::TAU * lo / SR).exp();
    let (mut s1, mut s2, mut s3) = (0.0f32, 0.0f32, 0.0f32);
    let mut acc = 0.0f64;
    for v in y {
        s1 += (v - s1) * a_hi;
        s2 += (s1 - s2) * a_hi;
        s3 += (s2 - s3) * a_lo;
        let b = s2 - s3;
        acc += f64::from(b * b);
    }
    (10.0 * (acc / y.len() as f64).max(1e-30).log10()) as f32
}

// 1. Each version sounds like itself.
#[test]
fn versions_sound_different() {
    let mut irs = Vec::new();
    for v in Version::ALL {
        let s = wet_only(v);
        let (l, r) = impulse_response(v, &s, 2.0);
        let m = mono(&l, &r);
        let rt = t20(&m);
        // Echo density in the first 80 ms: fraction of samples above -40 dB of the peak.
        let pk = peak(&m);
        let early = &m[..(0.08 * SR) as usize];
        let dense =
            early.iter().filter(|x| x.abs() > pk * 0.01).count() as f32 / early.len() as f32;
        let bright = band_rms(&m, 4000.0, 12_000.0) - band_rms(&m, 200.0, 800.0);
        println!(
            "{v:?}: T20 {rt:.2} s, density {dense:.2}, brightness {bright:.1} dB, peak {pk:.3}"
        );
        irs.push((v, rt, dense, bright));
    }
    let get = |v: Version| {
        irs.iter()
            .find(|x| x.0 == v)
            .copied()
            .unwrap_or((v, 0.0, 0.0, 0.0))
    };
    // Room is the shortest; Hall is longer than Room.
    assert!(get(Version::Room).1 < get(Version::Hall).1 * 0.5);
    // Plate is denser early than Hall, and brighter.
    assert!(get(Version::Plate).2 > get(Version::Hall).2);
    assert!(get(Version::Plate).3 > get(Version::Hall).3);
    // Spring is darker than Plate (band-limited transducers).
    assert!(get(Version::Spring).3 < get(Version::Plate).3);
}

// 2. Decay: long decay, long tail; short decay, short tail. And it matches the setting.
#[test]
fn decay_sets_the_tail_length() {
    for v in [Version::Clean, Version::Hall, Version::Plate] {
        for decay in [0.6f32, 2.4, 5.0] {
            let mut s = wet_only(v);
            let vs = &mut s.versions[v.index()];
            vs.decay_s = decay;
            vs.high_damp = 0.0;
            vs.low_damp = 0.0;
            vs.predelay_ms = 0.0;
            let (l, r) = impulse_response(v, &s, decay * 1.6 + 0.5);
            let rt = t20(&mono(&l, &r));
            println!("{v:?} decay {decay}: measured {rt:.2} s");
            assert!((rt / decay - 1.0).abs() < 0.3, "{v:?} {decay}: {rt}");
        }
    }
}

// 3. Pre-delay: nothing of the reverb before the pre-delay; the dry stays clear.
#[test]
fn predelay_keeps_the_start_clear() {
    for v in Version::ALL {
        let mut s = wet_only(v);
        s.versions[v.index()].predelay_ms = 80.0;
        let (l, r) = impulse_response(v, &s, 0.3);
        let m = mono(&l, &r);
        let before = peak(&m[..(0.078 * SR) as usize]);
        let after = peak(&m[(0.08 * SR) as usize..]);
        assert!(before < 1e-6, "{v:?}: {before}");
        assert!(after > 1e-3, "{v:?}");
    }
}

// 4. Damping: high damping rolls the top off the tail, low damping the bottom.
#[test]
fn damping_rolls_off_top_and_bottom() {
    for v in Version::ALL {
        let tail = |hd: f32, ld: f32| {
            let mut s = wet_only(v);
            let vs = &mut s.versions[v.index()];
            vs.high_damp = hd;
            vs.low_damp = ld;
            vs.predelay_ms = 0.0;
            let (l, r) = impulse_response(v, &s, 1.5);
            let m = mono(&l, &r);
            let t = &m[(0.3 * SR) as usize..];
            (band_rms(t, 4000.0, 12_000.0), band_rms(t, 60.0, 200.0))
        };
        let (hi0, lo0) = tail(0.0, 0.0);
        let (hi1, _) = tail(1.0, 0.0);
        let (_, lo1) = tail(0.0, 1.0);
        println!("{v:?}: highs {hi0:.1} -> {hi1:.1}, lows {lo0:.1} -> {lo1:.1}");
        assert!(hi1 < hi0 - 6.0, "{v:?} high damping");
        assert!(lo1 < lo0 - 3.0, "{v:?} low damping");
    }
}

// 5. Diffusion: low is echoey (sparse, peaky), high is smooth (dense).
#[test]
fn diffusion_low_is_echoey_high_is_smooth() {
    for v in [Version::Clean, Version::Hall, Version::Room, Version::Plate] {
        let crest = |d: f32| {
            let mut s = wet_only(v);
            let vs = &mut s.versions[v.index()];
            vs.diffusion = d;
            vs.predelay_ms = 0.0;
            if v.extra(Extra::Early).is_some() {
                vs.extras[v.extra(Extra::Early).unwrap_or(0)] = 0.0;
            }
            let (l, r) = impulse_response(v, &s, 0.5);
            let m = mono(&l, &r);
            let w = &m[(0.05 * SR) as usize..(0.25 * SR) as usize];
            peak(w) / rms(w)
        };
        let lo = crest(0.0);
        let hi = crest(1.0);
        println!("{v:?}: crest factor {lo:.1} (diffusion 0) vs {hi:.1} (diffusion 100)");
        assert!(lo > hi * 1.5, "{v:?}");
    }
}

// 6. Freeze holds the tail: level stays put, no loop clicks.
#[test]
fn freeze_holds_the_tail_clean() {
    for v in Version::ALL {
        let mut s = wet_only(v);
        let mut rv = Reverb::new(SR);
        let g = global(v);
        let src = noise((SR * 1.0) as usize, 0.3, 3);
        run(&mut rv, &src, &src, &g, &s);
        s.versions[v.index()].freeze = true;
        // Freeze while the input keeps playing: it must stop going in.
        let more = noise((SR * 6.0) as usize, 0.3, 9);
        let (l, r) = run(&mut rv, &more, &more, &g, &s);
        let m = mono(&l, &r);
        let sec = |k: usize| rms(&m[k * SR as usize..(k + 1) * SR as usize]);
        let (a, b) = (sec(1), sec(5));
        let change = lin_to_db(b) - lin_to_db(a);
        println!(
            "{v:?}: frozen level {:.1} dB -> {:.1} dB",
            lin_to_db(a),
            lin_to_db(b)
        );
        assert!(a > 1e-3, "{v:?} silent");
        let limit = if v == Version::Spring { 2.0 } else { 0.5 };
        assert!(change.abs() < limit, "{v:?}: {change}");
        // No clicks: no sample-to-sample jump far above the signal's own movement.
        let w = &m[SR as usize..];
        let d: Vec<f32> = w.windows(2).map(|p| p[1] - p[0]).collect();
        let jump = peak(&d) / rms(&d);
        assert!(jump < 8.0, "{v:?}: jump {jump}");
    }
}

// 8. Bypass: bit-identical to the input.
#[test]
fn bypass_is_bit_identical() {
    for v in Version::ALL {
        let s = Settings::default();
        let mut rv = Reverb::new(SR);
        let g = Global {
            bypass: true,
            ..global(v)
        };
        let l = noise(30_000, 0.5, 1);
        let r = noise(30_000, 0.5, 2);
        let (a, b) = run(&mut rv, &l, &r, &g, &s);
        assert_eq!(a, l, "{v:?}");
        assert_eq!(b, r, "{v:?}");
    }
}

// 9. Wet/Dry at 50% mix: parallel reverb, the dry signal intact underneath.
#[test]
fn half_mix_keeps_the_dry_intact() {
    for v in Version::ALL {
        let mut s = Settings::default();
        let vs = &mut s.versions[v.index()];
        vs.mix = 0.5;
        vs.dry_db = 0.0;
        vs.wet_db = 0.0;
        let x = phrase((SR * 2.0) as usize);
        let mut rv = Reverb::new(SR);
        let (l, _) = run(&mut rv, &x, &x, &global(v), &s);
        // The same with the dry level off: the difference is exactly half the input.
        let mut sw = s;
        sw.versions[v.index()].dry_db = -60.0;
        let mut rv = Reverb::new(SR);
        let (w, _) = run(&mut rv, &x, &x, &global(v), &sw);
        let err = l
            .iter()
            .zip(&w)
            .zip(&x)
            .map(|((y, wet), d)| (y - wet - 0.5 * d).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-6, "{v:?}: dry error {err}");
        assert!(rms(&w) > 1e-3, "{v:?}: no reverb");
        // And at Mix 0 the output is the input, bit for bit.
        let mut s0 = Settings::default();
        s0.versions[v.index()].mix = 0.0;
        let mut rv = Reverb::new(SR);
        let (l0, _) = run(&mut rv, &x, &x, &global(v), &s0);
        assert_eq!(l0, x, "{v:?} mix 0");
    }
}

// 12. Stereo link vs dual mono.
#[test]
fn link_is_wide_dual_mono_keeps_sides_apart() {
    for v in Version::ALL {
        let s = wet_only(v);
        let x = phrase((SR * 1.5) as usize);
        let silent = vec![0.0; x.len()];
        // Dual mono: input only on the left, nothing comes out on the right.
        let mut rv = Reverb::new(SR);
        let g = Global {
            link: false,
            ..global(v)
        };
        let (l, r) = run(&mut rv, &x, &silent, &g, &s);
        assert!(rms(&l) > 1e-3, "{v:?}");
        assert!(rms(&r) < 1e-6, "{v:?}: right {:e}", rms(&r));
        // Linked: a centred source gives a wide (decorrelated) tail.
        let mut rv = Reverb::new(SR);
        let (l, r) = run(&mut rv, &x, &x, &global(v), &s);
        let c: f32 = l.iter().zip(&r).map(|(a, b)| a * b).sum::<f32>()
            / (l.iter().map(|a| a * a).sum::<f32>() * r.iter().map(|b| b * b).sum::<f32>()).sqrt();
        println!("{v:?}: L/R correlation {c:.2}");
        assert!(c < 0.8, "{v:?}: {c}");
    }
}

// 15. Latency: measured, and reported (zero) for every version.
#[test]
fn latency_is_zero_and_measured() {
    for v in Version::ALL {
        let mut s = Settings::default();
        s.versions[v.index()].mix = 0.0;
        assert_eq!(latency_for(v, &s, SR), 0);
        // With the reverb in, the dry part still lines up: an impulse comes out at 0.
        let mut s = Settings::default();
        s.versions[v.index()].mix = 0.5;
        s.versions[v.index()].predelay_ms = 20.0;
        let (l, _) = impulse_response(v, &s, 0.05);
        let first = l.iter().position(|v| v.abs() > 1e-4).unwrap_or(usize::MAX);
        assert_eq!(first, 0, "{v:?}");
    }
}

// 16. Stereo and ping-pong wet/dry blend on their own; the dry stays intact.
#[test]
fn stereo_and_pingpong_blend_on_their_own() {
    let v = Version::Plate;
    let x = phrase((SR * 2.0) as usize);
    let silent = vec![0.0; x.len()];
    let corr = |s: &Settings| {
        let mut rv = Reverb::new(SR);
        let (l, r) = run(&mut rv, &x, &x, &global(v), s);
        let c = l.iter().zip(&r).map(|(a, b)| a * b).sum::<f32>()
            / (l.iter().map(|a| a * a).sum::<f32>() * r.iter().map(|b| b * b).sum::<f32>()).sqrt();
        (c, l, r)
    };
    // Width 0 at full stereo mix: mono.
    let mut s = wet_only(v);
    s.versions[v.index()].width = 0.0;
    let (c0, _, _) = corr(&s);
    // The same width with the stereo stage's wet/dry at 0: the plain (wide) reverb.
    s.versions[v.index()].stereo_mix = 0.0;
    let (c1, _, _) = corr(&s);
    println!("width 0: corr {c0:.2} at stereo mix 100%, {c1:.2} at 0%");
    assert!(c0 > 0.99);
    assert!(c1 < 0.8);
    // Ping-pong: input on the left only, bounce to the right appears with its mix up.
    let mut s = wet_only(v);
    let vs = &mut s.versions[v.index()];
    vs.pingpong_mix = 1.0;
    vs.pingpong_ms = 200.0;
    let g = Global {
        link: true,
        ..global(v)
    };
    let mut click = vec![0.0; (SR * 1.0) as usize];
    click[0] = 1.0;
    let silent1 = vec![0.0; click.len()];
    let mut rv = Reverb::new(SR);
    let (l, r) = run(&mut rv, &click, &silent1, &g, &s);
    let win = |y: &[f32], a: f32, b: f32| rms(&y[(a * SR) as usize..(b * SR) as usize]);
    // First bounce on the left around 200 ms, then the right around 400 ms.
    let l1 = win(&l, 0.2, 0.3);
    let r1 = win(&r, 0.2, 0.3);
    let r2 = win(&r, 0.4, 0.5);
    println!("ping-pong: L@200 {l1:.4}, R@200 {r1:.4}, R@400 {r2:.4}");
    assert!(l1 > r1 * 2.0);
    assert!(r2 > r1 * 1.5);
    // The dry is untouched by both stages.
    let mut s = Settings::default();
    let vs = &mut s.versions[v.index()];
    vs.mix = 0.0;
    vs.width = 2.0;
    vs.pingpong_mix = 1.0;
    let mut rv = Reverb::new(SR);
    let (l, _) = run(&mut rv, &x, &silent, &global(v), &s);
    assert_eq!(l, x);
}

// 17. Saturation warms the tail (adds harmonics), never clips harshly.
#[test]
fn saturation_warms_without_harsh_clipping() {
    let v = Version::Clean;
    let x = sine(220.0, 0.5, (SR * 1.5) as usize);
    let harm = |sat: f32| {
        let mut s = wet_only(v);
        let vs = &mut s.versions[v.index()];
        vs.saturation = sat;
        vs.decay_s = 0.4;
        vs.gain_in_db = 6.0;
        vs.extras[0] = 0.0; // no modulation: keep the tones exact
        let mut rv = Reverb::new(SR);
        let (l, _) = run(&mut rv, &x, &x, &global(v), &s);
        let t = &l[(0.5 * SR) as usize..];
        let h = amp_at(t, 440.0).max(amp_at(t, 660.0));
        (lin_to_db(h) - lin_to_db(amp_at(t, 220.0)), peak(t))
    };
    let (h0, p0) = harm(0.0);
    let (h1, p1) = harm(1.0);
    println!("harmonics {h0:.1} -> {h1:.1} dB, peak {p0:.2} -> {p1:.2}");
    assert!(h1 > h0 + 10.0);
    assert!(p1 < 2.0, "no runaway level");
}

// 18. Built-in EQ shapes the tail only.
#[test]
fn wet_eq_shapes_the_tail_only() {
    let v = Version::Hall;
    let x = noise((SR * 2.0) as usize, 0.2, 5);
    let lows = |cut: f32, mix: f32| {
        let mut s = Settings::default();
        let vs = &mut s.versions[v.index()];
        vs.low_cut_hz = cut;
        vs.mix = mix;
        vs.low_db = 0.0;
        let mut rv = Reverb::new(SR);
        let (l, _) = run(&mut rv, &x, &x, &global(v), &s);
        band_rms(&l[(0.5 * SR) as usize..], 40.0, 150.0)
    };
    let a = lows(20.0, 1.0);
    let b = lows(600.0, 1.0);
    println!("wet lows {a:.1} -> {b:.1} dB with the low cut");
    assert!(b < a - 10.0);
    // With mix 0 the low cut changes nothing.
    assert!((lows(20.0, 0.0) - lows(600.0, 0.0)).abs() < 1e-3);
    // Shelves and bell do something (on a bright version, so the top is there to shape).
    let v = Version::Clean;
    let mut s = wet_only(v);
    s.versions[v.index()].high_db = 12.0;
    let mut rv = Reverb::new(SR);
    let (l, _) = run(&mut rv, &x, &x, &global(v), &s);
    let mut s0 = wet_only(v);
    s0.versions[v.index()].high_db = 0.0;
    let mut rv0 = Reverb::new(SR);
    let (l0, _) = run(&mut rv0, &x, &x, &global(v), &s0);
    let t = (0.5 * SR) as usize;
    let (a, b) = (
        band_rms(&l[t..], 6000.0, 12_000.0),
        band_rms(&l0[t..], 6000.0, 12_000.0),
    );
    println!("high shelf +12 dB: highs {b:.1} -> {a:.1} dB");
    assert!(a > b + 4.0);
}

// 19. Built-in compressor tames the tail, the dry untouched.
#[test]
fn wet_compressor_tames_the_tail() {
    let v = Version::Room;
    let x = phrase((SR * 3.0) as usize);
    let level = |thr: f32| {
        let mut s = wet_only(v);
        let vs = &mut s.versions[v.index()];
        vs.threshold_db = thr;
        vs.ratio = 6.0;
        let mut rv = Reverb::new(SR);
        let (l, _) = run(&mut rv, &x, &x, &global(v), &s);
        peak(&l[SR as usize..])
    };
    let a = level(0.0);
    let b = level(-30.0);
    println!("wet peak {:.1} -> {:.1} dB", lin_to_db(a), lin_to_db(b));
    assert!(lin_to_db(b) < lin_to_db(a) - 6.0);
}

// Sample rates: decay holds at 44.1 and 96 kHz.
#[test]
fn decay_holds_across_sample_rates() {
    for sr in [44_100.0f32, 96_000.0] {
        let v = Version::Clean;
        let mut s = wet_only(v);
        let vs = &mut s.versions[v.index()];
        vs.decay_s = 2.0;
        vs.high_damp = 0.0;
        vs.low_damp = 0.0;
        let n = (3.5 * sr) as usize;
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let r = l.clone();
        let mut rv = Reverb::new(sr);
        let (a, b) = run(&mut rv, &l, &r, &global(v), &s);
        let m = mono(&a, &b);
        // Resample the EDC window to this rate.
        let win = (sr * 0.01) as usize;
        let mut acc = vec![0.0f64; m.len() + 1];
        let mut tail = 0.0;
        for i in (0..m.len()).rev() {
            tail += f64::from(m[i] * m[i]);
            acc[i] = tail;
        }
        let e: Vec<f32> = (0..m.len() / win)
            .map(|k| (10.0 * (acc[k * win] / acc[0]).log10()) as f32)
            .collect();
        let a5 = e.iter().position(|v| *v < -5.0).unwrap_or(0);
        let b25 = e.iter().position(|v| *v < -25.0).unwrap_or(e.len());
        let rt = (b25 - a5) as f32 * 0.03;
        assert!((rt / 2.0 - 1.0).abs() < 0.3, "{sr}: {rt}");
    }
}

// Stability: every version, longest decay, sweeping Size and Diffusion while it plays.
#[test]
fn stable_under_extremes_and_sweeps() {
    for v in Version::ALL {
        let mut rv = Reverb::new(SR);
        let x = noise((SR * 6.0) as usize, 0.5, 11);
        let mut s = wet_only(v);
        let g = global(v);
        let mut worst = 0.0f32;
        for (k, chunk) in x.chunks(4800).enumerate() {
            let vs = &mut s.versions[v.index()];
            vs.decay_s = 20.0;
            vs.size = (k as f32 * 0.17).sin() * 0.5 + 0.5;
            vs.diffusion = (k as f32 * 0.29).cos() * 0.5 + 0.5;
            vs.high_damp = 0.0;
            vs.low_damp = 0.0;
            let (l, r) = run(&mut rv, chunk, chunk, &g, &s);
            for y in l.iter().chain(&r) {
                assert!(y.is_finite(), "{v:?}");
                worst = worst.max(y.abs());
            }
        }
        println!("{v:?}: worst peak {worst:.2}");
        assert!(worst < 16.0, "{v:?}: {worst}");
    }
}

// The decay curve the display draws agrees with what the tank does.
#[test]
fn decay_curve_matches_the_tank() {
    let v = Version::Hall;
    let s = VersionSettings::defaults(v);
    let mid = curve::rt60(v, &s, Band::Mid, SR);
    let hi = curve::rt60(v, &s, Band::High, SR);
    let lo = curve::rt60(v, &s, Band::Low, SR);
    assert!((mid - s.decay_s).abs() < 1e-6);
    assert!(hi < mid && lo <= mid);
    assert!(curve::level_db(v, &s, Band::Mid, s.predelay_ms - 1.0, SR) <= -89.0);
    let at = curve::level_db(v, &s, Band::Mid, s.predelay_ms + mid * 1000.0, SR);
    assert!((at + 60.0).abs() < 1.5, "{at}");
}

#[test]
#[ignore]
fn level_report() {
    for v in Version::ALL {
        let s = wet_only(v);
        let x = noise((SR * 4.0) as usize, 0.25, 4);
        let ph = phrase((SR * 4.0) as usize);
        let mut rv = Reverb::new(SR);
        let (l, r) = run(&mut rv, &x, &x, &global(v), &s);
        let mut rv = Reverb::new(SR);
        let (pl, _) = run(&mut rv, &ph, &ph, &global(v), &s);
        let t = (2.0 * SR) as usize;
        println!(
            "{v:?}: noise in {:.1} dB -> wet {:.1}/{:.1} dB; phrase peak in {:.1} -> {:.1}",
            lin_to_db(rms(&x[t..])),
            lin_to_db(rms(&l[t..])),
            lin_to_db(rms(&r[t..])),
            lin_to_db(peak(&ph)),
            lin_to_db(peak(&pl[t..]))
        );
    }
}
