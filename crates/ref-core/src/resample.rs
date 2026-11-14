//! Offline sample-rate conversion for reference audio (windowed sinc).

const HALF_TAPS: i64 = 24;

/// Resamples `input` from `from_hz` to `to_hz`. Returns the input unchanged when the rates match.
#[must_use]
pub fn resample(input: &[f32], from_hz: f64, to_hz: f64) -> Vec<f32> {
    if !(from_hz > 0.0 && to_hz > 0.0) || (from_hz - to_hz).abs() < 1e-6 || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_hz / from_hz;
    let out_len = (input.len() as f64 * ratio).floor() as usize;
    // Cut-off in input samples: below the lower of the two Nyquists, with a little guard band.
    let cutoff = ratio.min(1.0) * 0.94;
    let mut out = Vec::with_capacity(out_len);
    let len = input.len() as i64;
    for n in 0..out_len {
        let t = n as f64 / ratio;
        let centre = t.floor() as i64;
        let mut acc = 0.0f64;
        let mut wsum = 0.0f64;
        for k in (centre - HALF_TAPS + 1)..=(centre + HALF_TAPS) {
            let x = t - k as f64;
            let arg = std::f64::consts::PI * x * cutoff;
            let sinc = if arg.abs() < 1e-12 {
                1.0
            } else {
                arg.sin() / arg
            };
            let w = x / HALF_TAPS as f64;
            let win = if w.abs() >= 1.0 {
                0.0
            } else {
                0.42 + 0.5 * (std::f64::consts::PI * w).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * w).cos()
            };
            let h = sinc * win;
            wsum += h;
            if (0..len).contains(&k) {
                acc += h * f64::from(input[k as usize]);
            }
        }
        out.push(if wsum.abs() > 1e-12 {
            (acc / wsum) as f32
        } else {
            0.0
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_keeps_its_frequency_and_level() {
        let f = 1000.0;
        let input: Vec<f32> = (0..44_100)
            .map(|i| (0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / 44_100.0).sin()) as f32)
            .collect();
        let out = resample(&input, 44_100.0, 48_000.0);
        assert_eq!(out.len(), 48_000);
        for (i, y) in out.iter().enumerate().skip(1000).take(5000) {
            let want = 0.5 * (2.0 * std::f64::consts::PI * f * i as f64 / 48_000.0).sin();
            assert!((f64::from(*y) - want).abs() < 2e-3, "{i}: {y} vs {want}");
        }
    }
}
