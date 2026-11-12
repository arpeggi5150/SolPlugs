//! PARTS: finds the sub, the kick and the snare in a track and outlines each one's spectrum.
//!
//! Method (our own, kept simple and explainable):
//! 1. A short STFT (2048 / 512) gives band powers every ~11 ms.
//! 2. Onset strength per region is the rise in level over the previous two frames.
//! 3. Kicks are onsets that rise mostly below 120 Hz; snares are onsets that rise both in the
//!    body (150-350 Hz) and in the snap (2-8 kHz) more than below 120 Hz.
//! 4. An element's outline is the average energy its hits *add* (frame after the hit minus the
//!    frame before), in the same band layout and dB reference as the tonal balance.
//! 5. The sub is the steady energy below 80 Hz in frames away from kicks.

use serde::{Deserialize, Serialize};

use crate::bands::{BANDS, bands_between, centre};
use crate::power_db;
use crate::profile::percentile;
use crate::spectrum::{BandAnalyzer, BandFrame};

const FFT: usize = 2048;
const HOP: usize = 512;
const FLOOR_DB: f64 = -120.0;

/// Which element a [`Part`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartKind {
    Sub,
    Kick,
    Snare,
}

impl PartKind {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sub => "Sub",
            Self::Kick => "Kick",
            Self::Snare => "Snare",
        }
    }

    /// All kinds, low to high.
    pub const ALL: [PartKind; 3] = [PartKind::Sub, PartKind::Kick, PartKind::Snare];
}

/// One detected element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub kind: PartKind,
    /// Whether the element was found with enough confidence to show.
    pub found: bool,
    /// Level of the element, dB relative to the track total (the tonal-balance reference).
    pub level_db: f64,
    /// Frequency span of the outline, Hz.
    pub lo_hz: f64,
    pub hi_hz: f64,
    /// Outline: per band dB (tonal-balance reference), [`FLOOR_DB`] outside the span.
    pub outline_db: Vec<f64>,
    /// Strongest frequency (fundamental region), Hz.
    pub peak_hz: f64,
    /// Hits per minute (0 for the sub).
    pub hits_per_min: f64,
    /// Median time for a hit to fall 12 dB, ms (0 for the sub).
    pub decay_ms: f64,
    /// Median rise at the hit, dB (0 for the sub).
    pub punch_db: f64,
}

impl Part {
    fn missing(kind: PartKind) -> Self {
        Self {
            kind,
            found: false,
            level_db: FLOOR_DB,
            lo_hz: 0.0,
            hi_hz: 0.0,
            outline_db: vec![FLOOR_DB; BANDS],
            peak_hz: 0.0,
            hits_per_min: 0.0,
            decay_ms: 0.0,
            punch_db: 0.0,
        }
    }
}

/// The elements found in a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Parts {
    pub elements: Vec<Part>,
}

impl Parts {
    /// The element of a kind, if analysed.
    #[must_use]
    pub fn get(&self, kind: PartKind) -> Option<&Part> {
        self.elements.iter().find(|p| p.kind == kind)
    }

    /// Repairs data read from disk.
    pub fn normalise(&mut self) {
        self.elements.truncate(PartKind::ALL.len());
        for p in &mut self.elements {
            p.outline_db.resize(BANDS, FLOOR_DB);
            for x in &mut p.outline_db {
                if !x.is_finite() {
                    *x = FLOOR_DB;
                }
                *x = x.clamp(-200.0, 50.0);
            }
            for x in [
                &mut p.level_db,
                &mut p.lo_hz,
                &mut p.hi_hz,
                &mut p.peak_hz,
                &mut p.hits_per_min,
                &mut p.decay_ms,
                &mut p.punch_db,
            ] {
                if !x.is_finite() {
                    *x = 0.0;
                }
            }
        }
    }
}

fn region_db(f: &BandFrame, r: &std::ops::Range<usize>) -> f64 {
    power_db(r.clone().map(|b| f.total(b)).sum())
}

/// Frames an onset may take to rise (the 2048-sample window smears a hit over several hops).
const RISE_FRAMES: usize = 3;

/// Onset strength: rise over the level [`RISE_FRAMES`] frames earlier, never negative.
fn onset_strength(env: &[f64]) -> Vec<f64> {
    (0..env.len())
        .map(|t| {
            if t < RISE_FRAMES {
                0.0
            } else {
                (env[t] - env[t - RISE_FRAMES]).max(0.0)
            }
        })
        .collect()
}

/// Peaks of `d` above a local median plus `margin_db`, at least `min_gap` frames apart.
fn pick_peaks(d: &[f64], margin_db: f64, min_gap: usize) -> Vec<usize> {
    let half = 40;
    let mut out: Vec<usize> = Vec::new();
    let mut win: Vec<f64> = Vec::with_capacity(2 * half + 1);
    for t in 1..d.len().saturating_sub(1) {
        if d[t] < d[t - 1] || d[t] < d[t + 1] || d[t] < margin_db {
            continue;
        }
        win.clear();
        win.extend_from_slice(&d[t.saturating_sub(half)..(t + half + 1).min(d.len())]);
        win.sort_by(f64::total_cmp);
        if d[t] < percentile(&win, 0.5) + margin_db {
            continue;
        }
        if let Some(&last) = out.last()
            && t - last < min_gap
        {
            if d[t] > d[last] {
                out.pop();
            } else {
                continue;
            }
        }
        out.push(t);
    }
    out
}

/// Hits whose level (just after the hit) is within `range_db` of the 90th percentile of all hits.
fn loud_hits(hits: Vec<usize>, env: &[f64], range_db: f64) -> Vec<usize> {
    let level = |t: usize| env[t].max(env.get(t + 1).copied().unwrap_or(env[t]));
    let mut levels: Vec<f64> = hits.iter().map(|&t| level(t)).collect();
    levels.sort_by(f64::total_cmp);
    let top = percentile(&levels, 0.9);
    hits.into_iter()
        .filter(|&t| level(t) >= top - range_db)
        .collect()
}

struct HitStats {
    outline: [f64; BANDS],
    /// Mean rise per band at the hits, dB (post over pre): high where the element dominates.
    rise_db: [f64; BANDS],
    punch_db: f64,
    decay_ms: f64,
}

fn hit_stats(
    frames: &[BandFrame],
    hits: &[usize],
    region: &std::ops::Range<usize>,
    rise: &[f64],
    hop_ms: f64,
) -> HitStats {
    let mut added = [0.0f64; BANDS];
    let mut rise_db = [0.0f64; BANDS];
    let mut decays = Vec::with_capacity(hits.len());
    let mut punches = Vec::with_capacity(hits.len());
    for &t in hits {
        let pre = t.saturating_sub(RISE_FRAMES);
        let post = (t + 1).min(frames.len() - 1);
        for b in 0..BANDS {
            let a = frames[post].total(b).max(frames[t].total(b)) - frames[pre].total(b);
            added[b] += a.max(0.0) / hits.len() as f64;
            let after = frames[post].total(b).max(frames[t].total(b));
            let before = frames[pre].total(b);
            rise_db[b] += (power_db(after + 1e-12) - power_db(before + 1e-12)) / hits.len() as f64;
        }
        punches.push(rise[t]);
        let peak = region_db(&frames[post], region).max(region_db(&frames[t], region));
        let mut k = post;
        while k + 1 < frames.len() && k - t < 100 && region_db(&frames[k], region) > peak - 12.0 {
            k += 1;
        }
        decays.push((k - t) as f64 * hop_ms);
    }
    punches.sort_by(f64::total_cmp);
    decays.sort_by(f64::total_cmp);
    let mut outline = [FLOOR_DB; BANDS];
    for b in 0..BANDS {
        outline[b] = power_db(added[b]);
    }
    HitStats {
        outline,
        rise_db,
        punch_db: percentile(&punches, 0.5),
        decay_ms: percentile(&decays, 0.5),
    }
}

/// How far below its peak an outline may fall and still count as part of the element.
const OUTLINE_RANGE_DB: f64 = 18.0;
/// A band belongs to a hit element only if it rises at least this much on the hits.
const MIN_RISE_DB: f64 = 3.0;

/// Turns an outline into a [`Part`]: around the peak of each anchor range, the contiguous
/// run of bands (inside `span`) that stay within [`OUTLINE_RANGE_DB`] of the strongest
/// anchor. Contiguity keeps spill from hats or bass notes that happen to coincide with hits
/// from stretching the outline; several anchors let a snare show its body and its snap.
/// `peak_hz` is the peak of the first anchor.
///
/// With `rise_db` (per-band rise at the hits), peaks are where the element stands out most
/// rather than where the mix is loudest, and bands that barely rise (a bass line or pad
/// that happens to sound on the hit) are left out.
fn finish(
    kind: PartKind,
    mut outline: [f64; BANDS],
    reference_db: f64,
    span: std::ops::Range<usize>,
    anchors: &[std::ops::Range<usize>],
    rise_db: Option<&[f64; BANDS]>,
) -> Part {
    for (b, x) in outline.iter_mut().enumerate() {
        *x = if span.contains(&b) {
            *x - reference_db
        } else {
            FLOOR_DB
        };
    }
    let score = |b: usize| rise_db.map_or(outline[b], |r| r[b]);
    let stands_out = |b: usize| rise_db.is_none_or(|r| r[b] >= MIN_RISE_DB);
    let peak_of = |r: &std::ops::Range<usize>| {
        r.clone()
            .filter(|b| span.contains(b))
            .max_by(|&a, &b| score(a).total_cmp(&score(b)))
    };
    let peaks: Vec<usize> = anchors.iter().filter_map(peak_of).collect();
    let top = peaks.iter().map(|&b| outline[b]).fold(FLOOR_DB, f64::max);
    let floor = top - OUTLINE_RANGE_DB;
    let mut keep = [false; BANDS];
    for &p in &peaks {
        if outline[p] <= FLOOR_DB || outline[p] < floor || !stands_out(p) {
            continue;
        }
        let ok = |b: usize| outline[b] >= floor && stands_out(b);
        let (mut lo, mut hi) = (p, p);
        while lo > span.start && ok(lo - 1) {
            lo -= 1;
        }
        while hi + 1 < span.end && ok(hi + 1) {
            hi += 1;
        }
        for k in &mut keep[lo..=hi] {
            *k = true;
        }
    }
    for (b, x) in outline.iter_mut().enumerate() {
        if !keep[b] {
            *x = FLOOR_DB;
        }
    }
    let peak_band = peaks.first().copied().unwrap_or(0);
    let kept: Vec<usize> = (0..BANDS).filter(|&b| keep[b]).collect();
    let energy: f64 = kept.iter().map(|&b| 10f64.powf(outline[b] / 10.0)).sum();
    Part {
        kind,
        found: !kept.is_empty(),
        level_db: power_db(energy),
        lo_hz: kept.first().map_or(0.0, |&b| crate::bands::lower_edge(b)),
        hi_hz: kept.last().map_or(0.0, |&b| crate::bands::upper_edge(b)),
        outline_db: outline.to_vec(),
        peak_hz: centre(peak_band),
        hits_per_min: 0.0,
        decay_ms: 0.0,
        punch_db: 0.0,
    }
}

/// Finds the sub, kick and snare in a stereo track.
#[must_use]
pub fn detect_parts(left: &[f32], right: &[f32], sample_rate: f64) -> Parts {
    let mut an = BandAnalyzer::new(sample_rate, FFT, HOP);
    let mut frames: Vec<BandFrame> = Vec::with_capacity(left.len() / HOP + 1);
    an.process(left, right, |f| frames.push(*f));
    let mut parts = Parts {
        elements: PartKind::ALL.iter().map(|&k| Part::missing(k)).collect(),
    };
    if frames.len() < 16 {
        return parts;
    }
    let hop_ms = 1000.0 * HOP as f64 / sample_rate;
    let minutes = frames.len() as f64 * hop_ms / 60_000.0;
    let mean_total = frames.iter().map(BandFrame::sum).sum::<f64>() / frames.len() as f64;
    let reference_db = power_db(mean_total);
    if reference_db < -70.0 {
        return parts;
    }

    let low = bands_between(40.0, 120.0);
    let body = bands_between(150.0, 350.0);
    let snap = bands_between(2000.0, 8000.0);
    let env = |r: &std::ops::Range<usize>| -> Vec<f64> {
        frames.iter().map(|f| region_db(f, r)).collect()
    };
    let (e_low, e_body, e_snap) = (env(&low), env(&body), env(&snap));
    let (d_low, d_body, d_snap) = (
        onset_strength(&e_low),
        onset_strength(&e_body),
        onset_strength(&e_snap),
    );
    let min_gap = (90.0 / hop_ms).ceil() as usize;

    let kicks: Vec<usize> = pick_peaks(&d_low, 4.0, min_gap)
        .into_iter()
        .filter(|&t| d_low[t] >= d_snap[t] + 1.0)
        .collect();
    let combined: Vec<f64> = (0..frames.len())
        .map(|t| d_body[t].min(d_snap[t]))
        .collect();
    let snares: Vec<usize> = pick_peaks(&combined, 3.0, min_gap)
        .into_iter()
        .filter(|&t| d_snap[t] > d_low[t] + 1.0 && d_body[t] >= 2.0)
        .collect();

    // Keep only hits near the loudest ones: ghost notes, hats and spill rise too, just quieter.
    let kicks = loud_hits(kicks, &e_low, 10.0);
    let body_and_snap: Vec<f64> = (0..frames.len())
        .map(|t| e_body[t].min(e_snap[t] + 6.0))
        .collect();
    let snares = loud_hits(snares, &body_and_snap, 10.0);

    if kicks.len() >= 4 {
        let s = hit_stats(&frames, &kicks, &low, &d_low, hop_ms);
        let mut p = finish(
            PartKind::Kick,
            s.outline,
            reference_db,
            bands_between(25.0, 10_000.0),
            &[bands_between(35.0, 150.0)],
            Some(&s.rise_db),
        );
        p.hits_per_min = kicks.len() as f64 / minutes.max(1e-9);
        p.punch_db = s.punch_db;
        p.decay_ms = s.decay_ms;
        parts.elements[1] = p;
    }
    if snares.len() >= 4 {
        let s = hit_stats(&frames, &snares, &body, &d_snap, hop_ms);
        let mut p = finish(
            PartKind::Snare,
            s.outline,
            reference_db,
            bands_between(140.0, 16_000.0),
            // The shell resonance (below ~160 Hz the rise is mostly bass notes on the
            // beat), then the wires.
            &[bands_between(160.0, 400.0), bands_between(2000.0, 8000.0)],
            Some(&s.rise_db),
        );
        p.hits_per_min = snares.len() as f64 / minutes.max(1e-9);
        p.punch_db = s.punch_db;
        p.decay_ms = s.decay_ms;
        parts.elements[2] = p;
    }

    // Sub: steady low energy away from kicks, analysed with a longer FFT for resolution.
    let near_kick = |t: usize| kicks.iter().any(|&k| t + 2 >= k && t <= k + 8);
    let mut sub_acc = [0.0f64; BANDS];
    let mut n = 0usize;
    for (t, f) in frames.iter().enumerate() {
        if !near_kick(t) {
            for b in 0..BANDS {
                sub_acc[b] += f.total(b);
            }
            n += 1;
        }
    }
    if n > 0 {
        for x in &mut sub_acc {
            *x = power_db(*x / n as f64);
        }
        let p = finish(
            PartKind::Sub,
            sub_acc,
            reference_db,
            bands_between(20.0, 80.0),
            &[bands_between(20.0, 80.0)],
            None,
        );
        // A sub that sits 35 dB under the track is not a sub.
        if p.level_db > -35.0 {
            parts.elements[0] = p;
        }
    }
    parts.normalise();
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 120 BPM: kick on 1 and 3, snare on 2 and 4, a steady 45 Hz sub, quiet hats.
    fn groove(sr: f64, secs: f64) -> Vec<f32> {
        let n = (sr * secs) as usize;
        let mut x: u32 = 7;
        let mut noise = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as f64 / u32::MAX as f64 - 0.5
        };
        let beat = (sr * 0.5) as usize;
        let mut out = vec![0.0f32; n];
        let mut kick_phase = 0.0f64;
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let pos = i % (beat * 2);
            let mut s = 0.15 * (2.0 * std::f64::consts::PI * 45.0 * t).sin();
            if pos < beat {
                let tk = pos as f64 / sr;
                let f = 55.0 + 90.0 * (-tk * 30.0).exp();
                kick_phase += 2.0 * std::f64::consts::PI * f / sr;
                s += 0.8 * (-tk * 12.0).exp() * kick_phase.sin();
            } else {
                kick_phase = 0.0;
                let ts = (pos - beat) as f64 / sr;
                let env = (-ts * 25.0).exp();
                s += env * (0.35 * (2.0 * std::f64::consts::PI * 200.0 * ts).sin() + 0.5 * noise());
            }
            if i % (beat / 2) < (sr * 0.02) as usize {
                s += 0.02 * noise();
            }
            *o = s as f32;
        }
        out
    }

    /// The groove under a busy mix: a bass line that re-triggers on every beat (so it rises
    /// with the snare) and a steady bright pad. The snare must still be placed at its body,
    /// not wherever the mix is loudest.
    #[test]
    fn snare_is_placed_at_its_body_in_a_dense_mix() {
        let sr = 48_000.0;
        let mut g = groove(sr, 16.0);
        let beat = (sr * 0.5) as usize;
        let mut x: u32 = 99;
        for (i, o) in g.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let note = if (i / beat).is_multiple_of(2) {
                98.0
            } else {
                123.0
            };
            let tb = (i % beat) as f64 / sr;
            let bass = 0.3 * (-tb * 4.0).exp() * (2.0 * std::f64::consts::PI * note * t).sin();
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let pad = 0.05 * (x as f64 / u32::MAX as f64 - 0.5);
            *o += (bass + pad) as f32;
        }
        let p = detect_parts(&g, &g, sr);
        let snare = p
            .get(PartKind::Snare)
            .cloned()
            .unwrap_or_else(|| Part::missing(PartKind::Snare));
        assert!(snare.found, "{p:#?}");
        assert!(
            (180.0..260.0).contains(&snare.peak_hz),
            "snare at {}",
            snare.peak_hz
        );
        assert!(
            snare.lo_hz >= 140.0,
            "snare outline starts at {}",
            snare.lo_hz
        );
    }

    #[test]
    fn finds_sub_kick_and_snare_in_a_groove() {
        let sr = 48_000.0;
        let g = groove(sr, 16.0);
        let p = detect_parts(&g, &g, sr);
        let kick = p
            .get(PartKind::Kick)
            .cloned()
            .unwrap_or_else(|| Part::missing(PartKind::Kick));
        let snare = p
            .get(PartKind::Snare)
            .cloned()
            .unwrap_or_else(|| Part::missing(PartKind::Snare));
        let sub = p
            .get(PartKind::Sub)
            .cloned()
            .unwrap_or_else(|| Part::missing(PartKind::Sub));
        assert!(kick.found && snare.found && sub.found, "{p:#?}");
        assert!(
            (kick.hits_per_min - 60.0).abs() < 8.0,
            "kick {}",
            kick.hits_per_min
        );
        assert!(
            (snare.hits_per_min - 60.0).abs() < 8.0,
            "snare {}",
            snare.hits_per_min
        );
        assert!(
            (35.0..160.0).contains(&kick.peak_hz),
            "kick at {}",
            kick.peak_hz
        );
        assert!(
            (120.0..400.0).contains(&snare.peak_hz),
            "snare at {}",
            snare.peak_hz
        );
        assert!(
            (30.0..70.0).contains(&sub.peak_hz),
            "sub at {}",
            sub.peak_hz
        );
        assert!(
            snare.hi_hz > 4000.0,
            "snare outline ends at {}",
            snare.hi_hz
        );
    }

    #[test]
    fn silence_finds_nothing() {
        let z = vec![0.0f32; 48_000 * 4];
        let p = detect_parts(&z, &z, 48_000.0);
        assert!(p.elements.iter().all(|e| !e.found));
    }
}
