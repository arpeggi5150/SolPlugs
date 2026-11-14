//! 4x oversampling for the coloured modes: two cascaded linear-phase half-band stages
//! (2x then 2x), polyphase so the zero taps cost nothing. The filters are Kaiser-windowed
//! sinc half-bands designed at start-up. Total latency is a whole number of base-rate
//! samples ([`Oversampler::LATENCY`]), so the dry path can be aligned exactly.

/// Zeroth-order modified Bessel function (series), for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1e-16 {
            break;
        }
    }
    sum
}

/// Half-band low-pass taps: length `4k + 3`, centre `2k + 1`. Returns the non-zero
/// even-index taps (`h[0], h[2], ... h[4k+2]`); the centre tap is 0.5.
fn halfband_even_taps(k: usize, beta: f64) -> Vec<f32> {
    let n = 4 * k + 3;
    let m = (n - 1) as f64 / 2.0;
    let denom = bessel_i0(beta);
    (0..n)
        .step_by(2)
        .map(|i| {
            let t = (i as f64 - m) / 2.0;
            let sinc = (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t);
            let r = (i as f64 - m) / m;
            let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
            (0.5 * sinc * w) as f32
        })
        .collect()
}

/// One 2x half-band stage: an upsampler and a downsampler sharing the same taps.
#[derive(Clone, Debug)]
struct Stage {
    /// Even-index taps.
    h: Vec<f32>,
    k: usize,
    /// Upsampler input history, newest first.
    up_hist: Vec<f32>,
    /// Downsampler even-phase history, newest first.
    dn_even: Vec<f32>,
    /// Downsampler odd-phase history, newest first.
    dn_odd: Vec<f32>,
}

impl Stage {
    fn new(k: usize, beta: f64) -> Self {
        let h = halfband_even_taps(k, beta);
        let n = h.len();
        Self {
            h,
            k,
            up_hist: vec![0.0; n],
            dn_even: vec![0.0; n],
            dn_odd: vec![0.0; k + 2],
        }
    }

    /// Delay of the up+down pair at the stage's high rate, samples.
    fn round_trip(&self) -> usize {
        2 * (2 * self.k + 1)
    }

    #[inline]
    fn push(hist: &mut [f32], x: f32) {
        hist.rotate_right(1);
        hist[0] = x;
    }

    /// One input sample in, two output samples (even, odd) out.
    #[inline]
    fn up(&mut self, x: f32) -> (f32, f32) {
        Self::push(&mut self.up_hist, x);
        let even: f32 = self
            .h
            .iter()
            .zip(&self.up_hist)
            .map(|(h, x)| h * x)
            .sum::<f32>()
            * 2.0;
        // The odd phase is only the centre tap (2 * 0.5 = 1): a pure delay of k samples.
        let odd = self.up_hist[self.k];
        (even, odd)
    }

    /// Two input samples (even, odd) in, one output sample out.
    #[inline]
    fn down(&mut self, even: f32, odd: f32) -> f32 {
        Self::push(&mut self.dn_even, even);
        let acc: f32 = self.h.iter().zip(&self.dn_even).map(|(h, x)| h * x).sum();
        // Centre tap: v[2p - (2k+1)], the odd sample from k+1 steps ago.
        let centre = self.dn_odd[self.k];
        Self::push(&mut self.dn_odd, odd);
        acc + 0.5 * centre
    }

    fn clear(&mut self) {
        self.up_hist.fill(0.0);
        self.dn_even.fill(0.0);
        self.dn_odd.fill(0.0);
    }
}

/// 4x oversampler: runs a function on each of four high-rate samples per input sample.
#[derive(Clone, Debug)]
pub struct Oversampler {
    s1: Stage,
    s2: Stage,
    /// One 2x-rate sample of delay so the total is a whole number of base samples.
    pad: f32,
}

/// Taps parameter of the first (base to 2x) stage: 47 taps.
const K1: usize = 11;
/// Taps parameter of the second (2x to 4x) stage: 19 taps.
const K2: usize = 4;

impl Default for Oversampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler {
    /// Base-rate latency, samples: stage 1 round trip (at 2x) plus stage 2 round trip and
    /// the pad (at 4x), both whole base samples.
    pub const LATENCY: usize = (2 * (2 * K1 + 1)) / 2 + (2 * (2 * K2 + 1) + 2) / 4;

    /// A new oversampler with silent history.
    pub fn new() -> Self {
        let s = Self {
            s1: Stage::new(K1, 8.0),
            s2: Stage::new(K2, 7.0),
            pad: 0.0,
        };
        debug_assert_eq!(
            Self::LATENCY * 4,
            s.s1.round_trip() * 2 + s.s2.round_trip() + 2
        );
        s
    }

    /// Processes one base-rate sample: upsample 4x, apply `f` to each high-rate sample,
    /// downsample.
    #[inline]
    pub fn process(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        let (a, b) = self.s1.up(x);
        let mut mid = [0.0f32; 2];
        for (slot, v) in mid.iter_mut().zip([a, b]) {
            let (c, d) = self.s2.up(v);
            *slot = self.s2.down(f(c), f(d));
        }
        // Pad by one 2x sample: shift the pair.
        let even = self.pad;
        let odd = mid[0];
        self.pad = mid[1];
        self.s1.down(even, odd)
    }

    /// Clears the filter history.
    pub fn clear(&mut self) {
        self.s1.clear();
        self.s2.clear();
        self.pad = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_lands_on_the_reported_latency() {
        let mut os = Oversampler::new();
        let out: Vec<f32> = (0..200)
            .map(|i| os.process(if i == 0 { 1.0 } else { 0.0 }, |v| v))
            .collect();
        let (peak_at, peak) = out.iter().enumerate().fold((0, 0.0f32), |acc, (i, &v)| {
            if v.abs() > acc.1 { (i, v.abs()) } else { acc }
        });
        assert_eq!(peak_at, Oversampler::LATENCY, "{out:?}");
        assert!((peak - 1.0).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn passes_audio_band_flat() {
        let sr = 48_000.0f32;
        for f in [100.0f32, 1000.0, 10_000.0, 18_000.0] {
            let mut os = Oversampler::new();
            let mut peak = 0.0f32;
            for i in 0..9600 {
                let y = os.process((std::f32::consts::TAU * f * i as f32 / sr).sin(), |v| v);
                if i > 4800 {
                    peak = peak.max(y.abs());
                }
            }
            assert!(
                (crate::util::lin_to_db(peak)).abs() < 0.1,
                "{f} Hz: {} dB",
                crate::util::lin_to_db(peak)
            );
        }
    }

    #[test]
    fn saturation_does_not_alias() {
        // A hard-driven 15 kHz sine through tanh: the 3rd harmonic (45 kHz) would fold to
        // 3 kHz without oversampling. Check nothing appears there.
        let sr = 48_000.0f32;
        let f = 15_000.0f32;
        let n = 48_000;
        let mut os = Oversampler::new();
        let y: Vec<f32> = (0..n)
            .map(|i| {
                os.process(
                    0.9 * (std::f32::consts::TAU * f * i as f32 / sr).sin(),
                    |v| (3.0 * v).tanh(),
                )
            })
            .collect();
        let mag = |freq: f32| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, v) in y.iter().enumerate().skip(4800) {
                let a = std::f64::consts::TAU * f64::from(freq) * i as f64 / f64::from(sr);
                re += f64::from(*v) * a.cos();
                im += f64::from(*v) * a.sin();
            }
            ((re * re + im * im).sqrt() / (n - 4800) as f64 * 2.0) as f32
        };
        let fund = mag(f);
        let alias = mag(3000.0);
        assert!(
            crate::util::lin_to_db(alias / fund) < -60.0,
            "alias {} dB",
            crate::util::lin_to_db(alias / fund)
        );
    }
}
