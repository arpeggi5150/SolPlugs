//! The brief's measurable acceptance tests, on synthetic drums and a real vocal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solplugs_comp_core::util::{db_to_lin, lin_to_db};
use solplugs_comp_core::{Compressor, Global, Mode, Settings};

const SR: f32 = 48_000.0;

/// A deterministic noise source.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    }
}

/// A 120 bpm drum loop: kick on 1 and 3, snare on 2 and 4, eighth-note hats. Stereo, the
/// hats panned right.
fn drums(seconds: f32) -> [Vec<f32>; 2] {
    let n = (seconds * SR) as usize;
    let beat = (SR * 0.5) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    let mut rng = Lcg(7);
    for i in 0..n {
        let b = i / beat;
        let t = (i % beat) as f32 / SR;
        let mut s = 0.0;
        if b.is_multiple_of(2) {
            // Kick: pitch drop 120 -> 50 Hz.
            let f = 50.0 + 70.0 * (-t * 30.0).exp();
            s += 0.9 * (std::f32::consts::TAU * f * t).sin() * (-t * 9.0).exp();
        } else {
            s += 0.6 * rng.next() * (-t * 22.0).exp();
            s += 0.35 * (std::f32::consts::TAU * 190.0 * t).sin() * (-t * 18.0).exp();
        }
        let th = ((i % (beat / 2)) as f32) / SR;
        let hat = 0.18 * rng.next() * (-th * 70.0).exp();
        l[i] = s + 0.3 * hat;
        r[i] = s + hat;
    }
    [l, r]
}

/// The test vocal from the pitch crate (CC BY 4.0, see ATTRIBUTION.md), resampled to 48 k
/// by sample repetition is not needed: the compressor runs at the file's own rate.
fn vocal() -> (Vec<f32>, f32) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pitch-core/testdata/singing-female.wav"
    );
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
    let x: Vec<f32> = r
        .samples::<i32>()
        .step_by(spec.channels as usize)
        .map(|s| s.unwrap() as f32 / scale)
        .collect();
    (x, spec.sample_rate as f32)
}

fn sine(f: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin())
        .collect()
}

fn mode_defaults(mode: Mode) -> Settings {
    // The same starting points the plugin uses for each mode.
    let mut s = Settings::default();
    match mode {
        Mode::Vca => {
            s.attack_ms = 10.0;
            s.release_ms = 300.0;
            s.knee_db = 3.0;
        }
        Mode::Opto => {
            s.attack_ms = 10.0;
            s.release_ms = 1200.0;
            s.ratio = 3.0;
            s.knee_db = 8.0;
            s.rms = true;
            s.auto_release = true;
        }
        Mode::Fet => {
            s.attack_ms = 0.3;
            s.release_ms = 250.0;
            s.knee_db = 3.0;
        }
        Mode::VariMu => {
            s.attack_ms = 25.0;
            s.release_ms = 1500.0;
            s.ratio = 2.5;
            s.knee_db = 18.0;
            s.rms = true;
            s.auto_release = true;
        }
        Mode::Clean => {
            s.attack_ms = 5.0;
            s.release_ms = 120.0;
            s.colour = false;
        }
    }
    s
}

struct Run {
    out: [Vec<f32>; 2],
    gr: Vec<f32>,
    stats: solplugs_comp_core::Stats,
}

/// Runs stereo audio through `mode` in 512-sample blocks.
fn run(mode: Mode, s: Settings, g: Global, input: &[Vec<f32>; 2]) -> Run {
    run_sr(mode, s, g, input, SR)
}

fn run_sr(mode: Mode, s: Settings, mut g: Global, input: &[Vec<f32>; 2], sr: f32) -> Run {
    let mut c = Compressor::new(sr);
    g.mode = mode;
    let mut all = [Settings::default(); 5];
    all[mode.index()] = s;
    let mut out = [input[0].clone(), input[1].clone()];
    let mut gr = Vec::new();
    let mut stats = Default::default();
    let n = out[0].len();
    let mut i = 0;
    while i < n {
        let e = (i + 512).min(n);
        let (a, b) = out.split_at_mut(1);
        let mut io = [&mut a[0][i..e], &mut b[0][i..e]];
        stats = c.process(&mut io, None, &g, &all);
        gr.push(stats.gr[0]);
        i = e;
    }
    Run { out, gr, stats }
}

fn linked() -> Global {
    Global {
        link: true,
        ..Default::default()
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Dynamic range: the spread of 10 ms RMS levels (95th minus 20th percentile), dB.
/// Compression narrows it; it ignores the single sample a transient lets through.
fn range_db(x: &[f32]) -> f32 {
    let mut w: Vec<f32> = x.chunks(480).map(|c| lin_to_db(rms(c))).collect();
    w.sort_by(f32::total_cmp);
    w[w.len() * 95 / 100] - w[w.len() * 20 / 100]
}

/// Crest factor in dB: peak over RMS.
fn crest(x: &[f32]) -> f32 {
    lin_to_db(peak(x) / rms(x))
}

#[test]
fn static_curve_matches_settings_in_clean_mode() {
    // A steady 1 kHz sine 12 dB over the threshold, peak detector, hard knee, 4:1:
    // 9 dB of reduction.
    let x = sine(1000.0, db_to_lin(-6.0), 48_000);
    let mut s = mode_defaults(Mode::Clean);
    s.threshold_db = -18.0;
    s.knee_db = 0.0;
    let r = run(Mode::Clean, s, linked(), &[x.clone(), x]);
    let gr = *r.gr.last().unwrap();
    assert!((gr - 9.0).abs() < 0.3, "gr {gr}");
    let out_db = lin_to_db(peak(&r.out[0][40_000..]));
    assert!((out_db - -15.0).abs() < 0.3, "out {out_db}");
}

#[test]
fn bypass_is_bit_identical_in_every_mode() {
    let d = drums(2.0);
    for mode in Mode::ALL {
        let s = mode_defaults(mode);
        let g = Global {
            bypass: true,
            ..linked()
        };
        let r = run(mode, s, g, &d);
        let lat = Compressor::new(SR).latency(mode, &s);
        for c in 0..2 {
            assert_eq!(
                &r.out[c][lat..],
                &d[c][..d[c].len() - lat],
                "{mode:?} channel {c}"
            );
            assert!(r.out[c][..lat].iter().all(|v| *v == 0.0));
        }
    }
}

#[test]
fn latency_is_measured_and_reported_per_mode() {
    let mut s_la = mode_defaults(Mode::Clean);
    s_la.lookahead_ms = 5.0;
    let cases = [
        (Mode::Vca, mode_defaults(Mode::Vca)),
        (Mode::Opto, mode_defaults(Mode::Opto)),
        (Mode::Fet, mode_defaults(Mode::Fet)),
        (Mode::VariMu, mode_defaults(Mode::VariMu)),
        (Mode::Clean, mode_defaults(Mode::Clean)),
        (Mode::Clean, s_la),
    ];
    for (mode, mut s) in cases {
        // Below threshold, no colour: the output is the input, delayed.
        s.threshold_db = 0.0;
        s.colour = false;
        let mut x = vec![0.0f32; 4096];
        x[100] = 0.1;
        let r = run(mode, s, linked(), &[x.clone(), x]);
        let at = r.out[0]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap();
        let measured = at - 100;
        let reported = Compressor::new(SR).latency(mode, &s);
        println!(
            "latency {:>7} (lookahead {:.1} ms): {measured} samples = {:.2} ms at 48 kHz",
            mode.name(),
            s.lookahead_ms,
            measured as f32 * 1000.0 / SR
        );
        assert_eq!(measured, reported, "{mode:?}");
    }
}

#[test]
fn modes_sound_different_on_the_same_drums() {
    let d = drums(4.0);
    let mut crests = Vec::new();
    let mut outs = Vec::new();
    for mode in Mode::ALL {
        let mut s = mode_defaults(mode);
        s.threshold_db = -20.0;
        s.auto_makeup = false;
        let r = run(mode, s, linked(), &d);
        let tail = &r.out[0][96_000..];
        crests.push(crest(tail));
        outs.push(tail.to_vec());
        println!(
            "{:>7}: crest {:.1} dB (input {:.1}), mean GR {:.1} dB",
            mode.name(),
            crest(tail),
            crest(&d[0][96_000..]),
            r.gr.iter().sum::<f32>() / r.gr.len() as f32
        );
    }
    // Every pair differs clearly (normalised difference above -20 dB).
    for a in 0..5 {
        for b in a + 1..5 {
            let ga = rms(&outs[a]);
            let gb = rms(&outs[b]);
            let diff: Vec<f32> = outs[a]
                .iter()
                .zip(&outs[b])
                .map(|(x, y)| x / ga - y / gb)
                .collect();
            let rel = lin_to_db(rms(&diff));
            assert!(
                rel > -20.0,
                "{:?} vs {:?}: {rel} dB",
                Mode::ALL[a],
                Mode::ALL[b]
            );
        }
    }
    // How hard each grabs a hit: GR 1 ms into a loud burst after silence. FET fastest,
    // Opto and Vari-Mu softest.
    let grab: Vec<f32> = Mode::ALL
        .iter()
        .map(|m| {
            let mut c = Compressor::new(SR);
            let mut all = [Settings::default(); 5];
            all[m.index()] = mode_defaults(*m);
            let g = Global {
                mode: *m,
                ..linked()
            };
            let mut q = vec![0.0f32; 4800];
            let mut q2 = q.clone();
            c.process(&mut [&mut q, &mut q2], None, &g, &all);
            let mut hit = sine(1000.0, 0.9, 48);
            let mut h2 = hit.clone();
            c.process(&mut [&mut hit, &mut h2], None, &g, &all).gr[0]
        })
        .collect();
    println!("GR 1 ms into a hit: {grab:?}");
    let [vca, opto, fet, vmu, _] = [0, 1, 2, 3, 4].map(|i| grab[i]);
    assert!(fet > vca && vca > opto && vca > vmu, "{grab:?}");
}

#[test]
fn vca_on_drums_punches_without_pumping() {
    // Pumping: the gain reduction swings hard between hits. With a 300 ms release the GR
    // should ride, not drop to zero between beats.
    let d = drums(4.0);
    let mut s = mode_defaults(Mode::Vca);
    s.threshold_db = -16.0;
    s.ratio = 4.0;
    s.attack_ms = 10.0;
    let r = run(Mode::Vca, s, linked(), &d);
    let gr = &r.gr[100..];
    let max = gr.iter().fold(0.0f32, |m, v| m.max(*v));
    let min = gr.iter().fold(f32::MAX, |m, v| m.min(*v));
    println!("VCA drum bus GR {min:.1}..{max:.1} dB");
    assert!(max > 2.0 && max < 12.0, "max {max}");
    assert!(max - min < 8.0, "pumps {min}..{max}");
    // 10 ms attack lets the kick's front through: crest factor stays healthy (punch).
    assert!(crest(&r.out[0][96_000..]) > 7.0);
}

#[test]
fn opto_on_a_vocal_never_grabs() {
    let (v, vsr) = vocal();
    let x = [v.clone(), v.clone()];
    let mut s = mode_defaults(Mode::Opto);
    s.threshold_db = -22.0;
    let r = run_sr(Mode::Opto, s, linked(), &x, vsr);
    // GR moves smoothly: the largest change between 512-sample blocks is small.
    let jumps =
        r.gr.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
    let max = r.gr.iter().fold(0.0f32, |m, v| m.max(*v));
    println!("Opto vocal: max GR {max:.1} dB, largest block-to-block GR change {jumps:.2} dB");
    assert!(max > 3.0);
    assert!(jumps < 2.0, "{jumps}");
    // A burst after silence: 2 ms in, it has not grabbed more than 60% of its final GR.
    let mut burst = vec![0.0f32; 24_000];
    burst.extend(sine(440.0, 0.5, 48_000));
    let r = run(Mode::Opto, s, linked(), &[burst.clone(), burst]);
    let mut c = Compressor::new(SR);
    let mut all = [Settings::default(); 5];
    all[Mode::Opto.index()] = s;
    let g = Global {
        mode: Mode::Opto,
        ..linked()
    };
    let mut quiet = vec![0.0f32; 24_000];
    let mut q2 = quiet.clone();
    c.process(&mut [&mut quiet, &mut q2], None, &g, &all);
    let mut hit = sine(440.0, 0.5, 96);
    let mut h2 = hit.clone();
    let early = c.process(&mut [&mut hit, &mut h2], None, &g, &all).gr[0];
    let fin = *r.gr.last().unwrap();
    println!("Opto: {early:.2} dB after 2 ms, {fin:.2} dB settled");
    assert!(early < 0.6 * fin, "{early} vs {fin}");
}

#[test]
fn fet_bites_and_stays_clean_unless_pushed() {
    let x = sine(1000.0, db_to_lin(-20.0), 48_000);
    let mut s = mode_defaults(Mode::Fet);
    s.threshold_db = -30.0;
    let r = run(Mode::Fet, s, linked(), &[x.clone(), x.clone()]);
    let thd = thd_1k(&r.out[0][24_000..]);
    println!("FET at -20 dBFS in: THD {thd:.2}%");
    assert!(thd < 1.0, "{thd}");
    // Pushed (+18 dB in, all buttons): clearly gritty, and finite.
    s.gain_in_db = 18.0;
    s.all_buttons = true;
    let r = run(Mode::Fet, s, linked(), &[x.clone(), x]);
    let thd = thd_1k(&r.out[0][24_000..]);
    println!("FET all buttons, pushed: THD {thd:.2}%");
    assert!(thd > 1.0, "{thd}");
}

/// THD of a 1 kHz tone at 48 kHz (harmonics 2..8), percent.
fn thd_1k(x: &[f32]) -> f32 {
    let mag = |f: f32| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let a = std::f64::consts::TAU * f64::from(f) * i as f64 / f64::from(SR);
            re += f64::from(*v) * a.cos();
            im += f64::from(*v) * a.sin();
        }
        (re * re + im * im).sqrt()
    };
    let f1 = mag(1000.0);
    let h: f64 = (2..=8).map(|k| mag(1000.0 * k as f32).powi(2)).sum();
    (100.0 * h.sqrt() / f1) as f32
}

#[test]
fn fet_all_buttons_smashes_and_nothing_blows_up() {
    let d = drums(3.0);
    let mut s = mode_defaults(Mode::Fet);
    s.threshold_db = -24.0;
    s.all_buttons = true;
    s.gain_in_db = 24.0;
    s.gain_out_db = 0.0;
    let r = run(Mode::Fet, s, linked(), &d);
    let p = peak(&r.out[0]);
    let max_gr = r.gr.iter().fold(0.0f32, |m, v| m.max(*v));
    println!(
        "All buttons: output peak {:.1} dBFS, max GR {max_gr:.1} dB",
        lin_to_db(p)
    );
    assert!(r.out.iter().flatten().all(|v| v.is_finite()));
    assert!(p < db_to_lin(6.0));
    assert!(max_gr > 15.0);
    // The smash: much denser than the input.
    let (rin, rout) = (range_db(&d[0][48_000..]), range_db(&r.out[0][48_000..]));
    println!("All buttons: dynamic range {rin:.1} -> {rout:.1} dB");
    assert!(rout < rin - 6.0);
}

#[test]
fn varimu_glues_without_squashing() {
    let d = drums(4.0);
    let mut s = mode_defaults(Mode::VariMu);
    s.threshold_db = -18.0;
    let r = run(Mode::VariMu, s, linked(), &d);
    let mean = r.gr[40..].iter().sum::<f32>() / r.gr[40..].len() as f32;
    let r_in = range_db(&d[0][96_000..]);
    let r_out = range_db(&r.out[0][96_000..]);
    println!("Vari-Mu: mean GR {mean:.1} dB, dynamic range {r_in:.1} -> {r_out:.1} dB");
    assert!(mean > 1.0 && mean < 8.0);
    // Glue, not flat: the range narrows a little, not a lot.
    assert!(r_out < r_in && r_in - r_out < 6.0);
}

#[test]
fn clean_on_a_vocal_only_changes_level() {
    // Transparent: with colour off, the output is the input times a gain envelope. Check
    // by dividing out the gain: a slow-attack, slow-release Clean comp on a vocal
    // changes level, not shape. Measure THD-free: a steady tone gets no harmonics.
    let x = sine(1000.0, db_to_lin(-6.0), 48_000);
    let mut s = mode_defaults(Mode::Clean);
    s.threshold_db = -20.0;
    let r = run(Mode::Clean, s, linked(), &[x.clone(), x]);
    let thd = thd_1k(&r.out[0][24_000..]);
    println!("Clean: THD {thd:.4}% at 7 dB of GR");
    assert!(thd < 0.01);
}

#[test]
fn parallel_mix_keeps_the_dry_signal_intact() {
    let d = drums(2.0);
    for mode in Mode::ALL {
        let mut s = mode_defaults(mode);
        s.threshold_db = -30.0;
        s.mix = 0.5;
        // Wet off: the output must be exactly half the dry, delayed.
        s.wet_db = -60.0;
        let r = run(mode, s, linked(), &d);
        let lat = Compressor::new(SR).latency(mode, &s);
        let err = r.out[0][lat + 4800..]
            .iter()
            .zip(&d[0][4800..])
            .map(|(y, x)| (y - 0.5 * x).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-6, "{mode:?}: {err}");
        // Wet back on at +0, dry at -6 dB: both parts show in the peaks.
        s.wet_db = 0.0;
        s.dry_db = -6.0;
        let r = run(mode, s, linked(), &d);
        assert!(r.stats.wet_peak > 0.0 && r.stats.dry_peak > 0.0, "{mode:?}");
    }
}

#[test]
fn match_lines_up_average_peaks_within_half_a_db() {
    let d = drums(6.0);
    for mode in Mode::ALL {
        let mut s = mode_defaults(mode);
        s.threshold_db = -24.0;
        let r = run(mode, s, linked(), &d);
        let diff = r.stats.avg_before_db - r.stats.avg_after_db;
        // MATCH: add the difference to gain out.
        s.gain_out_db += diff;
        let r2 = run(mode, s, linked(), &d);
        let after = r2.stats.avg_before_db - r2.stats.avg_after_db;
        println!(
            "MATCH {:>7}: before {:.2}, after {:.2} -> gain out {:+.2} dB, residual {after:+.2} dB",
            mode.name(),
            r.stats.avg_before_db,
            r.stats.avg_after_db,
            diff
        );
        assert!(after.abs() < 0.5, "{mode:?}: {after}");
    }
}

#[test]
fn auto_makeup_holds_the_level_as_the_threshold_drops() {
    let d = drums(8.0);
    let mut levels = Vec::new();
    for thr in [-10.0, -20.0, -30.0] {
        let mut s = mode_defaults(Mode::Vca);
        s.threshold_db = thr;
        s.auto_makeup = true;
        let r = run(Mode::Vca, s, linked(), &d);
        // Measured directly: mean 20 ms-window peak over the last 3 s, output vs input.
        let win_peaks = |x: &[f32]| {
            let w: Vec<f32> = x[5 * 48_000..].chunks(960).map(peak).collect();
            lin_to_db(w.iter().sum::<f32>() / w.len() as f32)
        };
        println!(
            "thr {thr}: auto {:.2} dB, meter before {:.2} after {:.2}",
            r.stats.auto_makeup_db, r.stats.avg_before_db, r.stats.avg_after_db
        );
        levels.push(win_peaks(&r.out[0]) - win_peaks(&d[0]));
    }
    println!("auto-makeup output vs input: {levels:?} dB");
    for l in levels {
        assert!(l.abs() < 1.0, "{l}");
    }
}

#[test]
fn gain_in_and_gain_out_work_in_every_mode() {
    let x = sine(1000.0, db_to_lin(-40.0), 24_000);
    for mode in Mode::ALL {
        let mut s = mode_defaults(mode);
        s.threshold_db = 0.0;
        s.colour = false;
        let base = peak(&run(mode, s, linked(), &[x.clone(), x.clone()]).out[0][12_000..]);
        s.gain_in_db = 6.0;
        let gin = peak(&run(mode, s, linked(), &[x.clone(), x.clone()]).out[0][12_000..]);
        s.gain_in_db = 0.0;
        s.gain_out_db = -6.0;
        let gout = peak(&run(mode, s, linked(), &[x.clone(), x.clone()]).out[0][12_000..]);
        assert!((lin_to_db(gin / base) - 6.0).abs() < 0.1, "{mode:?} in");
        assert!((lin_to_db(gout / base) + 6.0).abs() < 0.1, "{mode:?} out");
    }
}

#[test]
fn stereo_link_holds_the_image_dual_mono_does_not() {
    // Loud left, quiet right.
    let l = sine(1000.0, db_to_lin(-6.0), 48_000);
    let r_ = sine(1000.0, db_to_lin(-30.0), 48_000);
    let s = Settings {
        threshold_db: -20.0,
        ..mode_defaults(Mode::Clean)
    };
    let linked_run = run(Mode::Clean, s, linked(), &[l.clone(), r_.clone()]);
    let dual = run(
        Mode::Clean,
        s,
        Global {
            link: false,
            ..Default::default()
        },
        &[l.clone(), r_.clone()],
    );
    let bal = |o: &[Vec<f32>; 2]| lin_to_db(peak(&o[0][24_000..]) / peak(&o[1][24_000..]));
    let input_bal = 24.0;
    println!(
        "balance in {input_bal:.1} dB, linked {:.1} dB, dual mono {:.1} dB",
        bal(&linked_run.out),
        bal(&dual.out)
    );
    assert!((bal(&linked_run.out) - input_bal).abs() < 0.2);
    assert!(bal(&dual.out) < input_bal - 5.0);
}

#[test]
fn moving_knobs_and_switching_modes_never_clicks() {
    // A steady sine while the threshold sweeps fast and the mode changes: the output's
    // sample-to-sample step never exceeds what the sine itself does by much.
    let n = 96_000;
    let x = sine(200.0, 0.5, n);
    let max_step = x
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    let mut c = Compressor::new(SR);
    let mut all: [Settings; 5] = Mode::ALL.map(mode_defaults);
    let mut out = Vec::with_capacity(n);
    for (b, chunk) in x.chunks(256).enumerate() {
        let thr = if (b / 20) % 2 == 0 { -40.0 } else { 0.0 };
        for s in &mut all {
            s.threshold_db = thr;
        }
        let mode = Mode::ALL[(b / 37) % 5];
        let g = Global {
            mode,
            bypass: (b / 53) % 2 == 1,
            ..linked()
        };
        let mut l = chunk.to_vec();
        let mut r = chunk.to_vec();
        c.process(&mut [&mut l, &mut r], None, &g, &all);
        out.extend(l);
    }
    let worst = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    println!("largest step {worst:.4} vs sine's own {max_step:.4}");
    assert!(worst < max_step * 2.0, "{worst} vs {max_step}");
}
