//! DIFF: where a mix falls outside the range of its references, in numbers and in words.

use serde::{Deserialize, Serialize};

use crate::bands::{BANDS, centre, lower_edge, region_of, upper_edge};
use crate::group::{GroupStats, Span};
use crate::parts::PartKind;
use crate::profile::Profile;

/// Options for [`diff`].
#[derive(Clone, Copy, Debug)]
pub struct DiffOptions {
    /// dB of slack around the reference range before a band counts as outside.
    pub tolerance_db: f64,
    /// Runs whose worst band is closer than this are not reported.
    pub min_issue_db: f64,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            tolerance_db: 0.5,
            min_issue_db: 1.0,
        }
    }
}

/// One band's comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BandDiff {
    /// Mix minus the group mean, dB.
    pub to_mean: f64,
    /// How far outside the group range (0 inside, + above, - below), dB.
    pub outside: f64,
}

/// A run of neighbouring bands outside the range, described in words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub region: String,
    pub lo_hz: f64,
    pub hi_hz: f64,
    /// Worst signed distance outside the range, dB.
    pub worst_db: f64,
    pub text: String,
}

/// One scalar metric's comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricDiff {
    pub name: String,
    pub unit: String,
    pub value: f64,
    pub range: Span,
    pub outside: f64,
}

/// One element's comparison (PARTS).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartDiff {
    pub kind: PartKind,
    pub level_outside: Option<f64>,
    pub peak_hz: Option<f64>,
    pub peak_range: Option<Span>,
    pub text: Vec<String>,
}

/// The full comparison.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diff {
    pub bands: Vec<BandDiff>,
    pub issues: Vec<Issue>,
    pub metrics: Vec<MetricDiff>,
    pub parts: Vec<PartDiff>,
}

fn hz(x: f64) -> String {
    if x >= 1000.0 {
        format!("{:.1} kHz", x / 1000.0)
    } else {
        format!("{x:.0} Hz")
    }
}

/// Compares a mix against a group.
#[must_use]
pub fn diff(mix: &Profile, group: &GroupStats, opts: DiffOptions) -> Diff {
    let mut bands = vec![BandDiff::default(); BANDS];
    if !mix.tonal.is_empty() {
        for (b, d) in bands.iter_mut().enumerate() {
            if let Some(Some(s)) = group.tonal.get(b) {
                let v = mix.tonal.mean_db[b];
                *d = BandDiff {
                    to_mean: v - s.mean,
                    outside: s.outside(v, opts.tolerance_db),
                };
            }
        }
    }
    Diff {
        issues: issues(&bands, opts),
        metrics: metrics(mix, group, opts),
        parts: parts(mix, group, opts),
        bands,
    }
}

fn issues(bands: &[BandDiff], opts: DiffOptions) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut b = 0;
    while b < bands.len() {
        let sign = bands[b].outside.signum();
        if bands[b].outside == 0.0 {
            b += 1;
            continue;
        }
        let start = b;
        while b < bands.len() && bands[b].outside != 0.0 && bands[b].outside.signum() == sign {
            b += 1;
        }
        let run = start..b;
        let worst_band = run
            .clone()
            .max_by(|&x, &y| bands[x].outside.abs().total_cmp(&bands[y].outside.abs()))
            .unwrap_or(start);
        let worst = bands[worst_band].outside;
        if worst.abs() < opts.min_issue_db || run.len() < 2 {
            continue;
        }
        let region = region_of(centre(worst_band)).name.to_string();
        let (lo, hi) = (lower_edge(start), upper_edge(b - 1));
        let dir = if worst > 0.0 { "above" } else { "below" };
        out.push(Issue {
            text: format!(
                "{region}: {:.1} dB {dir} the references ({}-{})",
                worst.abs(),
                hz(lo),
                hz(hi)
            ),
            region,
            lo_hz: lo,
            hi_hz: hi,
            worst_db: worst,
        });
    }
    out.sort_by(|a, b| b.worst_db.abs().total_cmp(&a.worst_db.abs()));
    out
}

fn metrics(mix: &Profile, g: &GroupStats, opts: DiffOptions) -> Vec<MetricDiff> {
    let rows: [(&str, &str, Option<f64>, Option<Span>, f64); 7] = [
        (
            "Loudness",
            "LUFS",
            mix.integrated_lufs,
            g.integrated_lufs,
            opts.tolerance_db,
        ),
        (
            "Loudness range",
            "LU",
            mix.loudness_range,
            g.loudness_range,
            0.5,
        ),
        (
            "True peak",
            "dBTP",
            Some(mix.true_peak_db),
            g.true_peak_db,
            0.2,
        ),
        ("Peak to loudness", "dB", mix.plr_db, g.plr_db, 0.5),
        ("Crest factor", "dB", Some(mix.crest_db), g.crest_db, 0.5),
        ("Width", "", mix.width, g.width, 0.03),
        ("Correlation", "", mix.correlation, g.correlation, 0.03),
    ];
    rows.into_iter()
        .filter_map(|(name, unit, v, s, tol)| {
            let (v, s) = (v?, s?);
            Some(MetricDiff {
                name: name.into(),
                unit: unit.into(),
                value: v,
                range: s,
                outside: s.outside(v, tol),
            })
        })
        .collect()
}

fn parts(mix: &Profile, g: &GroupStats, opts: DiffOptions) -> Vec<PartDiff> {
    g.parts
        .iter()
        .filter(|ps| ps.found_in > 0)
        .map(|ps| {
            let mine = mix.parts.get(ps.kind).filter(|p| p.found);
            let mut text = Vec::new();
            let label = ps.kind.label();
            let level_outside = mine
                .zip(ps.level_db)
                .map(|(p, s)| s.outside(p.level_db, opts.tolerance_db));
            match (mine, level_outside) {
                (None, _) => text.push(format!(
                    "{label}: not found in the mix (found in {} of {} references)",
                    ps.found_in, g.count
                )),
                (Some(_), Some(o)) if o.abs() >= opts.min_issue_db => {
                    text.push(format!(
                        "{label}: {:.1} dB {} the references",
                        o.abs(),
                        if o > 0.0 {
                            "louder than"
                        } else {
                            "quieter than"
                        }
                    ));
                }
                _ => {}
            }
            if let (Some(p), Some(s)) = (mine, ps.peak_hz) {
                // Peak frequencies are band centres, so allow one band of slack.
                let slack = p.peak_hz * (2f64.powf(1.0 / 6.0) - 1.0);
                if s.outside(p.peak_hz, slack) != 0.0 {
                    text.push(format!(
                        "{label}: centred at {} (references {}-{})",
                        hz(p.peak_hz),
                        hz(s.lo),
                        hz(s.hi)
                    ));
                }
            }
            if let (Some(p), Some(s)) = (mine, ps.decay_ms) {
                let o = s.outside(p.decay_ms, 15.0);
                if o != 0.0 {
                    text.push(format!(
                        "{label}: decays in {:.0} ms (references {:.0}-{:.0} ms)",
                        p.decay_ms, s.lo, s.hi
                    ));
                }
            }
            PartDiff {
                kind: ps.kind,
                level_outside,
                peak_hz: mine.map(|p| p.peak_hz),
                peak_range: ps.peak_hz,
                text,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::group_stats;
    use crate::profile::TonalBalance;

    fn flat(level: f64) -> Profile {
        Profile {
            tonal: TonalBalance {
                mean_db: vec![level; BANDS],
                low_db: vec![level - 3.0; BANDS],
                high_db: vec![level + 3.0; BANDS],
                width: vec![0.5; BANDS],
            },
            integrated_lufs: Some(-9.0),
            ..Profile::default()
        }
    }

    #[test]
    fn a_bump_outside_the_group_is_reported_in_words() {
        let refs = [flat(-20.0), flat(-21.0), flat(-19.0)];
        let g = group_stats(&refs.iter().collect::<Vec<_>>()).unwrap_or_else(unreachable_group);
        let mut mix = flat(-20.0);
        for b in 20..26 {
            mix.tonal.mean_db[b] = -15.0;
        }
        mix.integrated_lufs = Some(-14.0);
        let d = diff(&mix, &g, DiffOptions::default());
        assert_eq!(d.bands[22].outside, 3.5);
        assert_eq!(d.bands[10].outside, 0.0);
        assert_eq!(d.issues.len(), 1);
        assert!(
            d.issues[0].text.starts_with("Low-mids: 3.5 dB above"),
            "{}",
            d.issues[0].text
        );
        let loud = d
            .metrics
            .iter()
            .find(|m| m.name == "Loudness")
            .map_or(0.0, |m| m.outside);
        assert!((loud + 4.5).abs() < 1e-9);
    }

    fn unreachable_group() -> GroupStats {
        GroupStats {
            count: 0,
            tonal: vec![],
            band_width: vec![],
            integrated_lufs: None,
            loudness_range: None,
            true_peak_db: None,
            plr_db: None,
            crest_db: None,
            rms_db: None,
            correlation: None,
            width: None,
            parts: vec![],
        }
    }
}
