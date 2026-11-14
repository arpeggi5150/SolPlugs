//! The brief's acceptance tests that the DSP alone can check. (MATCH, per-style memory
//! and the UI ones live in the app crate's tests.)

use solplugs_eq_core::style::{api, maag, massive, neve, pultec, ssl};
use solplugs_eq_core::{
    CleanBand, Eq, Global, Place, Settings, Shape, Style, latency_for, util::lin_to_db,
};

const SR: f32 = 48_000.0;

fn sine(f: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin())
        .collect()
}

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

/// Runs a stereo signal through in blocks of 128.
fn run(eq: &mut Eq, l: &[f32], r: &[f32], g: &Global, s: &Settings) -> (Vec<f32>, Vec<f32>) {
    let mut a = l.to_vec();
    let mut b = r.to_vec();
    for (ca, cb) in a.chunks_mut(128).zip(b.chunks_mut(128)) {
        eq.process(&mut [ca, cb], g, s);
    }
    (a, b)
}

fn global(style: Style) -> Global {
    Global {
        style,
        bypass: false,
        link: true,
    }
}

/// Amplitude of frequency `f` in `y` (from `skip` on), by correlation.
fn amp_at(y: &[f32], f: f32, skip: usize) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in y.iter().enumerate().skip(skip) {
        let ph = std::f64::consts::TAU * f64::from(f) * i as f64 / f64::from(SR);
        re += f64::from(*v) * ph.sin();
        im += f64::from(*v) * ph.cos();
    }
    ((re * re + im * im).sqrt() * 2.0 / (y.len() - skip) as f64) as f32
}

/// Gain of the EQ at `f` (sine in, sine out), dB.
fn gain_at(style: Style, s: &Settings, f: f32) -> f32 {
    let mut eq = Eq::new(SR);
    let x = sine(f, 0.1, 24_000);
    let (y, _) = run(&mut eq, &x, &x, &global(style), s);
    lin_to_db(amp_at(&y, f, 12_000) / 0.1)
}

#[test]
fn defaults_are_flat_in_every_style() {
    let s = Settings::default();
    for st in Style::ALL {
        for f in [60.0, 1000.0, 8000.0] {
            let g = gain_at(st, &s, f);
            assert!(g.abs() < 0.1, "{} at {f}: {g:.2} dB", st.name());
        }
    }
}

#[test]
fn t01_pultec_boost_and_cut_trick() {
    let mut s = Settings::default();
    let k = &mut s.styles[Style::Pultec.index()].knobs;
    k[pultec::LOW_FREQ] = 2.0;
    k[pultec::LOW_BOOST] = 10.0;
    k[pultec::LOW_ATTEN] = 10.0;
    k[pultec::HIGH_BOOST] = 5.0;
    let low = gain_at(Style::Pultec, &s, 45.0);
    let dip = gain_at(Style::Pultec, &s, 220.0);
    let air = gain_at(Style::Pultec, &s, 10_000.0);
    assert!(low > 2.5, "lows {low}");
    assert!(dip < -2.0, "low-mid dip {dip}");
    assert!(air > 6.0, "air {air}");
}

#[test]
fn t02_api_band_narrows_as_you_boost() {
    // At an octave from the centre, a small boost keeps more of its gain than a big one.
    let share = |g: f32| {
        let mut s = Settings::default();
        let k = &mut s.styles[Style::Api.index()].knobs;
        k[api::MID_FREQ] = 3.0; // 1.5k
        k[api::MID_GAIN] = g;
        gain_at(Style::Api, &s, 3000.0) / gain_at(Style::Api, &s, 1500.0)
    };
    let (small, big) = (share(4.0), share(12.0));
    assert!(
        big < small - 0.15,
        "share an octave up: small {small:.2}, big {big:.2}"
    );
}

#[test]
fn t03_ssl_clean_carve() {
    let mut s = Settings::default();
    let k = &mut s.styles[Style::Ssl.index()].knobs;
    k[ssl::LMF_GAIN] = -6.0;
    k[ssl::LMF_FREQ] = 400.0;
    k[ssl::LMF_Q] = 2.0;
    let at = gain_at(Style::Ssl, &s, 400.0);
    assert!((at + 6.0).abs() < 1.6, "cut {at}");
    assert!(gain_at(Style::Ssl, &s, 3000.0).abs() < 0.3);
    assert!(gain_at(Style::Ssl, &s, 60.0).abs() < 0.4);
}

#[test]
fn t04_neve_warm_colour() {
    // Pushed, the Neve stage adds harmonics; Clean adds none.
    let thd = |style: Style| {
        let mut s = Settings::default();
        s.styles[style.index()].common.gain_in_db = 12.0;
        s.styles[style.index()].common.gain_out_db = -12.0;
        let mut eq = Eq::new(SR);
        let x = sine(200.0, 0.5, 48_000);
        let (y, _) = run(&mut eq, &x, &x, &global(style), &s);
        let f1 = amp_at(&y, 200.0, 24_000);
        let h = amp_at(&y, 400.0, 24_000) + amp_at(&y, 600.0, 24_000);
        lin_to_db(h / f1)
    };
    let neve = thd(Style::Neve);
    let clean = thd(Style::Clean);
    assert!(neve > -45.0, "Neve harmonics {neve:.1} dB");
    assert!(clean < -100.0, "Clean harmonics {clean:.1} dB");
    // And the low shelf really lifts the lows.
    let mut s = Settings::default();
    s.styles[Style::Neve.index()].knobs[neve::LOW_GAIN] = 8.0;
    assert!(gain_at(Style::Neve, &s, 50.0) > 6.0);
}

#[test]
fn t05_clean_surgical_notch_changes_nothing_else() {
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        shape: Shape::Notch,
        freq: 1000.0,
        q: 8.0,
        ..CleanBand::default()
    };
    assert!(gain_at(Style::Clean, &s, 1000.0) < -40.0);
    for f in [100.0, 500.0, 2000.0, 8000.0] {
        let g = gain_at(Style::Clean, &s, f);
        assert!(g.abs() < 0.15, "{f}: {g}");
    }
}

#[test]
fn t08_parallel_mix_keeps_the_dry() {
    // 50% mix with a deep notch: the dry half still passes at the notch frequency.
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        shape: Shape::Notch,
        freq: 1000.0,
        q: 4.0,
        ..CleanBand::default()
    };
    s.styles[Style::Clean.index()].common.mix = 0.5;
    let g = gain_at(Style::Clean, &s, 1000.0);
    assert!((g + 6.02).abs() < 0.2, "{g}");
}

#[test]
fn t09_bypass_is_bit_identical() {
    for style in Style::ALL {
        let mut s = Settings::default();
        s.styles[style.index()].common.gain_in_db = 6.0;
        s.clean.linear = true;
        let mut eq = Eq::new(SR);
        let x = noise(20_000, 0.5, 7);
        let g = Global {
            style,
            bypass: true,
            link: true,
        };
        let (y, _) = run(&mut eq, &x, &x, &g, &s);
        let lat = latency_for(style, &s, SR);
        for i in 0..x.len() - lat {
            assert_eq!(
                y[i + lat].to_bits(),
                x[i].to_bits(),
                "{} at {i}",
                style.name()
            );
        }
    }
}

#[test]
fn t11_stereo_link_with_a_dynamic_band() {
    // Loud 2 kHz on the left only; a dynamic cut at 2 kHz. Linked, the quiet right side is
    // cut too (the image holds); dual mono, the right side is left alone.
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        freq: 2000.0,
        q: 1.0,
        dynamic: true,
        threshold_db: -30.0,
        range_db: -12.0,
        ..CleanBand::default()
    };
    let l = sine(2000.0, 0.5, 48_000);
    let r = sine(2000.0, 0.01, 48_000);
    let right_gain = |link: bool| {
        let mut eq = Eq::new(SR);
        let g = Global {
            style: Style::Clean,
            bypass: false,
            link,
        };
        let (_, y) = run(&mut eq, &l, &r, &g, &s);
        lin_to_db(amp_at(&y, 2000.0, 24_000) / 0.01)
    };
    let linked = right_gain(true);
    let dual = right_gain(false);
    assert!(linked < -8.0, "linked {linked}");
    assert!(dual.abs() < 0.3, "dual mono {dual}");
}

#[test]
fn t13_auto_makeup_holds_the_level() {
    let mut s = Settings::default();
    let k = &mut s.styles[Style::Ssl.index()].knobs;
    k[ssl::HMF_GAIN] = 10.0;
    k[ssl::HMF_FREQ] = 2000.0;
    s.styles[Style::Ssl.index()].common.auto_makeup = true;
    let mut eq = Eq::new(SR);
    let x = sine(2000.0, 0.1, 5 * 48_000);
    let (y, _) = run(&mut eq, &x, &x, &global(Style::Ssl), &s);
    let g = lin_to_db(amp_at(&y, 2000.0, 4 * 48_000) / 0.1);
    assert!(g.abs() < 1.0, "auto-makeup left {g:.2} dB");
}

#[test]
fn t14_gain_in_and_out_work_in_every_style() {
    for style in Style::ALL {
        for (gin, gout) in [(6.0f32, 0.0f32), (0.0, -6.0)] {
            let mut s = Settings::default();
            let c = &mut s.styles[style.index()].common;
            c.gain_in_db = gin;
            c.gain_out_db = gout;
            c.colour = false;
            let g = gain_at(style, &s, 1000.0);
            assert!(
                (g - (gin + gout)).abs() < 0.15,
                "{} in {gin} out {gout}: {g}",
                style.name()
            );
        }
    }
}

#[test]
fn t15_latency_is_what_we_report() {
    for (style, linear) in Style::ALL
        .iter()
        .map(|s| (*s, false))
        .chain([(Style::Clean, true)])
    {
        let mut s = Settings::default();
        s.clean.linear = linear;
        s.styles[style.index()].common.colour = false;
        let lat = latency_for(style, &s, SR);
        let mut eq = Eq::new(SR);
        let mut x = vec![0.0f32; lat + 4000];
        x[100] = 1.0;
        let (y, _) = run(&mut eq, &x, &x, &global(style), &s);
        let at = y
            .iter()
            .enumerate()
            .fold(
                (0, 0.0f32),
                |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a },
            )
            .0;
        assert_eq!(at, 100 + lat, "{} linear {linear}", style.name());
    }
}

#[test]
fn t16_dynamic_band_only_cuts_past_the_threshold() {
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        freq: 3000.0,
        q: 1.5,
        dynamic: true,
        threshold_db: -30.0,
        range_db: -9.0,
        ..CleanBand::default()
    };
    let quiet = {
        let mut eq = Eq::new(SR);
        let x = sine(3000.0, 0.01, 48_000); // -40 dBFS
        let (y, _) = run(&mut eq, &x, &x, &global(Style::Clean), &s);
        lin_to_db(amp_at(&y, 3000.0, 24_000) / 0.01)
    };
    let loud = {
        let mut eq = Eq::new(SR);
        let x = sine(3000.0, 0.5, 48_000); // -6 dBFS
        let (y, _) = run(&mut eq, &x, &x, &global(Style::Clean), &s);
        lin_to_db(amp_at(&y, 3000.0, 24_000) / 0.5)
    };
    assert!(quiet.abs() < 0.2, "quiet {quiet}");
    assert!((loud + 9.0).abs() < 0.5, "loud {loud}");
}

#[test]
fn t18_solo_plays_only_what_the_band_touches() {
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        freq: 1000.0,
        q: 4.0,
        gain_db: 6.0,
        solo: true,
        ..CleanBand::default()
    };
    let at = |f: f32| gain_at(Style::Clean, &s, f);
    assert!(at(1000.0).abs() < 0.5, "{}", at(1000.0));
    assert!(at(100.0) < -25.0);
    assert!(at(10_000.0) < -25.0);
}

#[test]
fn t19_massive_is_big_and_smooth() {
    let mut s = Settings::default();
    let k = &mut s.styles[Style::Massive.index()].knobs;
    k[massive::gain(0)] = 6.0;
    k[massive::gain(3)] = 4.0;
    let lo = gain_at(Style::Massive, &s, 40.0);
    let hi = gain_at(Style::Massive, &s, 15_000.0);
    let mid = gain_at(Style::Massive, &s, 1000.0);
    assert!(lo > 4.0 && hi > 2.5, "lo {lo} hi {hi}");
    assert!(mid.abs() < 1.0, "mid {mid}");
}

#[test]
fn t20_maag_air_without_harshness() {
    let mut s = Settings::default();
    s.styles[Style::Maag.index()].knobs[maag::AIR_GAIN] = 10.0;
    let at = |f| gain_at(Style::Maag, &s, f);
    let (a5, a10, a16) = (at(5000.0), at(10_000.0), at(16_000.0));
    assert!(a16 > 3.0, "16k {a16}");
    // A smooth rising slope, no presence peak.
    assert!(a5 < a10 && a10 < a16, "{a5} {a10} {a16}");
    assert!(at(2000.0) < 1.0);
}

#[test]
fn t21_curve_is_bold() {
    let mut s = Settings::default();
    let k = &mut s.styles[Style::Curve.index()].knobs;
    k[1] = 6.0;
    k[6] = 6.0;
    assert!(gain_at(Style::Curve, &s, 50.0) > 4.0);
    assert!(gain_at(Style::Curve, &s, 12_000.0) > 4.0);
}

#[test]
fn styles_sound_different_on_the_same_source() {
    // The same "+6 dB lows, +6 dB highs" intent on every hardware style gives different
    // curves: compare responses at a few points.
    let mut curves = vec![];
    for style in [
        Style::Pultec,
        Style::Api,
        Style::Ssl,
        Style::Neve,
        Style::Massive,
        Style::Curve,
    ] {
        let mut s = Settings::default();
        let k = &mut s.styles[style.index()].knobs;
        match style {
            Style::Pultec => {
                k[pultec::LOW_BOOST] = 4.5;
                k[pultec::HIGH_BOOST] = 4.0;
            }
            Style::Api => {
                k[api::LOW_GAIN] = 6.0;
                k[api::HIGH_GAIN] = 6.0;
            }
            Style::Ssl => {
                k[ssl::LF_GAIN] = 6.0;
                k[ssl::HF_GAIN] = 6.0;
            }
            Style::Neve => {
                k[neve::LOW_GAIN] = 6.0;
                k[neve::HIGH_GAIN] = 6.0;
            }
            Style::Massive => {
                k[massive::gain(0)] = 6.0;
                k[massive::gain(3)] = 6.0;
            }
            _ => {
                k[1] = 6.0;
                k[6] = 6.0;
            }
        }
        let c: Vec<f32> = [40.0, 150.0, 500.0, 3000.0, 12_000.0, 18_000.0]
            .iter()
            .map(|f| gain_at(style, &s, *f))
            .collect();
        curves.push((style, c));
    }
    for i in 0..curves.len() {
        for j in i + 1..curves.len() {
            let d: f32 = curves[i]
                .1
                .iter()
                .zip(&curves[j].1)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                d > 0.75,
                "{} vs {}: max difference {d:.2} dB",
                curves[i].0.name(),
                curves[j].0.name()
            );
        }
    }
}

#[test]
fn no_clicks_on_style_switch_bypass_or_knob_moves() {
    let mut s = Settings::default();
    s.styles[Style::Api.index()].knobs[api::MID_GAIN] = 8.0;
    let n = 48_000;
    let x = sine(440.0, 0.4, n);
    let mut y = x.clone();
    let mut y2 = x.clone();
    let mut eq = Eq::new(SR);
    let mut g = global(Style::Pultec);
    for (blk, (ca, cb)) in y.chunks_mut(64).zip(y2.chunks_mut(64)).enumerate() {
        match blk {
            150 => g.style = Style::Api,
            250 => g.bypass = true,
            350 => g.bypass = false,
            450 => g.style = Style::Clean,
            500..=600 => {
                let t = (blk - 500) as f32 / 100.0;
                s.clean.bands[0] = CleanBand {
                    used: true,
                    freq: 200.0 * 40f32.powf(t),
                    gain_db: 12.0,
                    q: 2.0,
                    ..CleanBand::default()
                };
            }
            _ => {}
        }
        eq.process(&mut [ca, cb], &g, &s);
    }
    // The biggest sample-to-sample step: a 440 Hz sine at 0.4 (up to ~+12 dB) moves at
    // most ~0.09 per sample; a click would be far larger.
    let worst = y
        .windows(2)
        .skip(1000)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.2, "largest step {worst}");
}

#[test]
fn coloured_styles_do_not_alias() {
    // A pushed 9 kHz sine: without oversampling the 3rd harmonic (27 kHz) would fold to
    // 21 kHz and the 5th (45 kHz) to 3 kHz.
    for style in [Style::Api, Style::Neve, Style::Curve, Style::Pultec] {
        let mut s = Settings::default();
        s.styles[style.index()].common.gain_in_db = 9.0;
        let mut eq = Eq::new(SR);
        let x = sine(9000.0, 0.5, 48_000);
        let (y, _) = run(&mut eq, &x, &x, &global(style), &s);
        let fund = amp_at(&y, 9000.0, 24_000);
        // (Above 20 kHz the half-band filters' transition lets a little fold back; the
        // audible band is what counts.)
        let alias = amp_at(&y, 3000.0, 24_000).max(amp_at(&y, 15_000.0, 24_000));
        let db = lin_to_db(alias / fund);
        assert!(db < -60.0, "{}: alias {db:.1} dB", style.name());
    }
}

#[test]
fn clean_linear_phase_matches_zero_latency_magnitude() {
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        freq: 800.0,
        gain_db: 8.0,
        q: 1.2,
        ..CleanBand::default()
    };
    s.clean.bands[1] = CleanBand {
        used: true,
        shape: Shape::HighShelf,
        freq: 6000.0,
        gain_db: -5.0,
        q: 0.7,
        place: Place::Mid,
        ..CleanBand::default()
    };
    for f in [200.0, 800.0, 3000.0, 9000.0] {
        let zl = gain_at(Style::Clean, &s, f);
        let mut sl = s;
        sl.clean.linear = true;
        let mut eq = Eq::new(SR);
        let x = sine(f, 0.1, 48_000);
        let (y, _) = run(&mut eq, &x, &x, &global(Style::Clean), &sl);
        let lp = lin_to_db(amp_at(&y, f, 24_000) / 0.1);
        assert!(
            (zl - lp).abs() < 0.25,
            "{f}: zero-latency {zl:.2}, linear {lp:.2}"
        );
    }
}

#[test]
fn mid_side_bands_touch_only_their_signal() {
    // A side-only cut leaves a mono (mid-only) signal untouched.
    let mut s = Settings::default();
    s.clean.bands[0] = CleanBand {
        used: true,
        freq: 1000.0,
        gain_db: -12.0,
        place: Place::Side,
        ..CleanBand::default()
    };
    assert!(gain_at(Style::Clean, &s, 1000.0).abs() < 0.05);
    // ...and cuts a pure side signal.
    let mut eq = Eq::new(SR);
    let l = sine(1000.0, 0.1, 48_000);
    let r: Vec<f32> = l.iter().map(|v| -v).collect();
    let (y, _) = run(&mut eq, &l, &r, &global(Style::Clean), &s);
    let g = lin_to_db(amp_at(&y, 1000.0, 24_000) / 0.1);
    assert!((g + 12.0).abs() < 0.2, "{g}");
}
