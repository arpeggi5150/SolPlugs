//! Group referencing: the range a set of references spans, band by band and metric by metric.

use serde::{Deserialize, Serialize};

use crate::bands::BANDS;
use crate::parts::PartKind;
use crate::profile::Profile;

/// Lowest, mean and highest value across the group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub lo: f64,
    pub mean: f64,
    pub hi: f64,
}

impl Span {
    /// The span of `values`, or `None` when there are none.
    #[must_use]
    pub fn of(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        let (mut lo, mut hi, mut sum, mut n) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0usize);
        for v in values.into_iter().filter(|v| v.is_finite()) {
            lo = lo.min(v);
            hi = hi.max(v);
            sum += v;
            n += 1;
        }
        (n > 0).then(|| Self {
            lo,
            mean: sum / n as f64,
            hi,
        })
    }

    /// How far `v` lies outside the span widened by `tolerance` on each side: 0 inside,
    /// positive above, negative below.
    #[must_use]
    pub fn outside(&self, v: f64, tolerance: f64) -> f64 {
        if v > self.hi + tolerance {
            v - (self.hi + tolerance)
        } else if v < self.lo - tolerance {
            v - (self.lo - tolerance)
        } else {
            0.0
        }
    }
}

/// The group's range for one element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSpan {
    pub kind: PartKind,
    /// References in which the element was found.
    pub found_in: usize,
    pub outline: Vec<Option<Span>>,
    pub level_db: Option<Span>,
    pub peak_hz: Option<Span>,
    pub decay_ms: Option<Span>,
    pub punch_db: Option<Span>,
}

/// What a group of references spans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupStats {
    pub count: usize,
    pub tonal: Vec<Option<Span>>,
    pub band_width: Vec<Option<Span>>,
    pub integrated_lufs: Option<Span>,
    pub loudness_range: Option<Span>,
    pub true_peak_db: Option<Span>,
    pub plr_db: Option<Span>,
    pub crest_db: Option<Span>,
    pub rms_db: Option<Span>,
    pub correlation: Option<Span>,
    pub width: Option<Span>,
    pub parts: Vec<PartSpan>,
}

/// Computes the group's spans. `None` for an empty group.
#[must_use]
pub fn group_stats(profiles: &[&Profile]) -> Option<GroupStats> {
    if profiles.is_empty() {
        return None;
    }
    let with_tonal: Vec<&&Profile> = profiles.iter().filter(|p| !p.tonal.is_empty()).collect();
    let band = |f: &dyn Fn(&Profile, usize) -> f64| -> Vec<Option<Span>> {
        (0..BANDS)
            .map(|b| Span::of(with_tonal.iter().map(|p| f(p, b))))
            .collect()
    };
    let parts = PartKind::ALL
        .iter()
        .map(|&kind| {
            let found: Vec<_> = profiles
                .iter()
                .filter_map(|p| p.parts.get(kind))
                .filter(|e| e.found && e.outline_db.len() == BANDS)
                .collect();
            PartSpan {
                kind,
                found_in: found.len(),
                outline: (0..BANDS)
                    .map(|b| {
                        Span::of(
                            found
                                .iter()
                                .map(|e| e.outline_db[b])
                                .filter(|&x| x > -119.0),
                        )
                    })
                    .collect(),
                level_db: Span::of(found.iter().map(|e| e.level_db)),
                peak_hz: Span::of(found.iter().map(|e| e.peak_hz)),
                decay_ms: Span::of(found.iter().map(|e| e.decay_ms).filter(|&x| x > 0.0)),
                punch_db: Span::of(found.iter().map(|e| e.punch_db).filter(|&x| x > 0.0)),
            }
        })
        .collect();
    Some(GroupStats {
        count: profiles.len(),
        tonal: band(&|p, b| p.tonal.mean_db[b]),
        band_width: band(&|p, b| p.tonal.width[b]),
        integrated_lufs: Span::of(profiles.iter().filter_map(|p| p.integrated_lufs)),
        loudness_range: Span::of(profiles.iter().filter_map(|p| p.loudness_range)),
        true_peak_db: Span::of(profiles.iter().map(|p| p.true_peak_db)),
        plr_db: Span::of(profiles.iter().filter_map(|p| p.plr_db)),
        crest_db: Span::of(profiles.iter().map(|p| p.crest_db)),
        rms_db: Span::of(profiles.iter().map(|p| p.rms_db)),
        correlation: Span::of(profiles.iter().filter_map(|p| p.correlation)),
        width: Span::of(profiles.iter().filter_map(|p| p.width)),
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_and_outside() {
        let s = Span::of([1.0, 3.0, 2.0, f64::NAN]).unwrap_or_default();
        assert_eq!((s.lo, s.mean, s.hi), (1.0, 2.0, 3.0));
        assert_eq!(s.outside(2.0, 0.0), 0.0);
        assert_eq!(s.outside(4.0, 0.5), 0.5);
        assert_eq!(s.outside(0.0, 0.5), -0.5);
        assert!(Span::of([]).is_none());
        assert!(group_stats(&[]).is_none());
    }
}
