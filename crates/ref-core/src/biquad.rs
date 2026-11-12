//! A direct-form-II-transposed biquad, and the two ITU-R BS.1770 K-weighting stages.

/// Normalised biquad coefficients (a0 = 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

/// One biquad's state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Filters one sample.
    #[inline]
    pub fn tick(&mut self, c: &Coeffs, x: f64) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }

    /// Clears the filter memory.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// BS.1770 stage 1: the high-shelf "pre-filter" (head effects), recomputed for any rate.
#[must_use]
pub fn k_shelf(sample_rate: f64) -> Coeffs {
    let f0 = 1_681.974_450_955_533;
    let gain_db = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (std::f64::consts::PI * f0 / sample_rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    Coeffs {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// BS.1770 stage 2: the "RLB" high-pass, recomputed for any rate.
#[must_use]
pub fn k_highpass(sample_rate: f64) -> Coeffs {
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (std::f64::consts::PI * f0 / sample_rate).tan();
    let a0 = 1.0 + k / q + k * k;
    Coeffs {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficients_at_48k_match_the_published_table() {
        // BS.1770-4, table 1 and 2 (48 kHz).
        let s = k_shelf(48_000.0);
        assert!((s.b0 - 1.535_124_859_586_97).abs() < 1e-9);
        assert!((s.b1 + 2.691_696_189_406_38).abs() < 1e-9);
        assert!((s.b2 - 1.198_392_810_852_85).abs() < 1e-9);
        assert!((s.a1 + 1.690_659_293_182_41).abs() < 1e-9);
        assert!((s.a2 - 0.732_480_774_215_85).abs() < 1e-9);
        let h = k_highpass(48_000.0);
        assert!((h.a1 + 1.990_047_454_833_98).abs() < 1e-9);
        assert!((h.a2 - 0.990_072_250_366_21).abs() < 1e-9);
    }
}
