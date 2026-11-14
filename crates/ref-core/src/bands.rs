//! The 1/6-octave band layout used for tonal balance, and named frequency regions.

/// Number of tonal-balance bands (1/6 octave, 20 Hz to 20 kHz).
pub const BANDS: usize = 60;
/// Lowest band edge in Hz.
pub const LOW_HZ: f64 = 20.0;
/// Bands per octave.
pub const PER_OCTAVE: f64 = 6.0;

/// Lower edge of band `i` in Hz.
#[must_use]
pub fn lower_edge(i: usize) -> f64 {
    LOW_HZ * 2f64.powf(i as f64 / PER_OCTAVE)
}

/// Upper edge of band `i` in Hz.
#[must_use]
pub fn upper_edge(i: usize) -> f64 {
    lower_edge(i + 1)
}

/// Geometric centre of band `i` in Hz.
#[must_use]
pub fn centre(i: usize) -> f64 {
    LOW_HZ * 2f64.powf((i as f64 + 0.5) / PER_OCTAVE)
}

/// The band containing `hz`, if it is inside 20 Hz..20 kHz.
#[must_use]
pub fn band_of(hz: f64) -> Option<usize> {
    if hz.is_nan() || hz < LOW_HZ {
        return None;
    }
    let i = ((hz / LOW_HZ).log2() * PER_OCTAVE).floor() as usize;
    (i < BANDS).then_some(i)
}

/// Bands whose centre lies in `lo_hz..hi_hz`.
#[must_use]
pub fn bands_between(lo_hz: f64, hi_hz: f64) -> std::ops::Range<usize> {
    let start = (0..BANDS).find(|&i| centre(i) >= lo_hz).unwrap_or(BANDS);
    let end = (0..BANDS).find(|&i| centre(i) > hi_hz).unwrap_or(BANDS);
    start..end.max(start)
}

/// A named frequency region, used to describe differences in words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub name: &'static str,
    pub lo_hz: f64,
    pub hi_hz: f64,
}

/// The regions, low to high.
pub const REGIONS: [Region; 8] = [
    Region {
        name: "Sub",
        lo_hz: 20.0,
        hi_hz: 60.0,
    },
    Region {
        name: "Bass",
        lo_hz: 60.0,
        hi_hz: 150.0,
    },
    Region {
        name: "Low-mids",
        lo_hz: 150.0,
        hi_hz: 400.0,
    },
    Region {
        name: "Mids",
        lo_hz: 400.0,
        hi_hz: 1000.0,
    },
    Region {
        name: "Upper-mids",
        lo_hz: 1000.0,
        hi_hz: 2500.0,
    },
    Region {
        name: "Presence",
        lo_hz: 2500.0,
        hi_hz: 5000.0,
    },
    Region {
        name: "Brilliance",
        lo_hz: 5000.0,
        hi_hz: 10_000.0,
    },
    Region {
        name: "Air",
        lo_hz: 10_000.0,
        hi_hz: 20_000.0,
    },
];

/// The region a frequency falls in.
#[must_use]
pub fn region_of(hz: f64) -> &'static Region {
    REGIONS
        .iter()
        .find(|r| hz < r.hi_hz)
        .unwrap_or(&REGIONS[REGIONS.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_spans_20_hz_to_20_khz() {
        assert!((lower_edge(0) - 20.0).abs() < 1e-9);
        assert!((upper_edge(BANDS - 1) - 20_480.0).abs() < 1.0);
        assert_eq!(band_of(1000.0), Some(33));
        assert_eq!(band_of(10.0), None);
        assert_eq!(region_of(300.0).name, "Low-mids");
    }
}
