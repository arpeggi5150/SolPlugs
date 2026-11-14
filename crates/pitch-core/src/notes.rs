//! Graph mode: a captured take analysed into notes, note edits (pitch, timing, splits),
//! and rendering the edited take with the same formant-preserving PSOLA as the live path.
//!
//! Times are in samples from the start of the take. Every note has a source span (where
//! it was sung) and a destination span (where it plays after timing edits). The time map
//! is piecewise linear through the notes' span ends, so stretching a note squeezes or
//! extends its neighbour (or the gap after it) and nothing else moves.

use serde::{Deserialize, Serialize};

use crate::detect::Mpm;
use crate::ola::{Grain, add_grain, align_mark, normalise};
use crate::realtime::{Config, Range};
use crate::scale::{Scale, freq_to_midi};

/// One analysis frame of a take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackFrame {
    /// Detected pitch (MIDI), 0 when unvoiced.
    pub midi: f32,
    /// Window RMS.
    pub rms: f32,
}

/// An analysis mark (pitch-synchronous grain centre).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// Position, samples.
    pub pos: i64,
    /// Distance to the next mark, samples.
    pub spacing: u32,
    /// Voiced (pitched) or not.
    pub voiced: bool,
}

/// A take's analysis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    /// Sample rate, Hz.
    pub sample_rate: f32,
    /// Frame hop, samples. Frame `i` is centred on `i * hop`.
    pub hop: usize,
    /// The pitch track.
    pub frames: Vec<TrackFrame>,
    /// Grain marks, ascending.
    pub marks: Vec<Mark>,
    /// Take length, samples.
    pub len: i64,
}

impl Analysis {
    /// Detected pitch at sample `t` (nearest frame), 0 if unvoiced.
    pub fn midi_at(&self, t: i64) -> f32 {
        let i = ((t as f64 / self.hop as f64).round().max(0.0) as usize)
            .min(self.frames.len().saturating_sub(1));
        self.frames.get(i).map_or(0.0, |f| f.midi)
    }
}

/// One note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Where it was sung: first sample.
    pub src_start: i64,
    /// Where it was sung: end (exclusive).
    pub src_end: i64,
    /// Where it plays: first sample.
    pub dst_start: i64,
    /// Where it plays: end (exclusive).
    pub dst_end: i64,
    /// Sung pitch centre (MIDI, fractional).
    pub pitch: f32,
    /// Pitch edit, semitones.
    pub shift: f32,
}

impl Note {
    /// Pitch it plays at.
    pub fn target(&self) -> f32 {
        self.pitch + self.shift
    }
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Analyses a take (mono) for graph mode: pitch track, notes' raw material and grain marks.
pub fn analyse(x: &[f32], sample_rate: f32, range: Range) -> Analysis {
    let cfg = Config { sample_rate, range };
    let hop = cfg.hop();
    let win = cfg.window();
    let max_p = cfg.max_period();
    // Same analysis signal as the live path: DC and rumble removed.
    let a = (-std::f32::consts::TAU * 50.0 / sample_rate).exp();
    let (mut px, mut py) = (0f32, 0f32);
    let hp: Vec<f32> = x
        .iter()
        .map(|&v| {
            py = a * (py + v - px);
            px = v;
            py
        })
        .collect();
    let mut mpm = Mpm::new(win, cfg.min_period(), max_p);
    let nframes = x.len() / hop + 1;
    let mut buf = vec![0f32; win];
    let mut raw: Vec<(f32, f32, f32)> = Vec::with_capacity(nframes);
    let mut prev: Option<f32> = None;
    let gate = 10f32.powf(-55.0 / 20.0);
    for i in 0..nframes {
        let c = (i * hop) as i64;
        for (j, b) in buf.iter_mut().enumerate() {
            let t = c - (win / 2) as i64 + j as i64;
            *b = if t >= 0 && (t as usize) < hp.len() {
                hp[t as usize]
            } else {
                0.0
            };
        }
        let d = mpm.detect_near(&buf, prev);
        let thr = if prev.is_some() { 0.6 } else { 0.8 };
        let voiced = d.period > 0.0 && d.clarity >= thr && d.rms >= gate;
        prev = voiced.then_some(d.period);
        raw.push((if voiced { d.period } else { 0.0 }, d.clarity, d.rms));
    }
    // Median of five over voiced neighbours; single unvoiced frames inside a voiced run
    // are bridged, lone voiced frames dropped.
    let mut frames = Vec::with_capacity(nframes);
    for i in 0..nframes {
        let lo = i.saturating_sub(2);
        let hi = (i + 3).min(nframes);
        let mut ps: Vec<f32> = raw[lo..hi]
            .iter()
            .map(|r| r.0)
            .filter(|p| *p > 0.0)
            .collect();
        let voiced_here = raw[i].0 > 0.0
            || (i > 0 && i + 1 < nframes && raw[i - 1].0 > 0.0 && raw[i + 1].0 > 0.0);
        let isolated = raw[i].0 > 0.0
            && (i == 0 || raw[i - 1].0 == 0.0)
            && (i + 1 >= nframes || raw[i + 1].0 == 0.0);
        let midi = if voiced_here && !isolated && ps.len() >= 2 {
            freq_to_midi(sample_rate / median(&mut ps))
        } else {
            0.0
        };
        frames.push(TrackFrame {
            midi,
            rms: raw[i].2,
        });
    }
    let mut an = Analysis {
        sample_rate,
        hop,
        frames,
        marks: Vec::new(),
        len: x.len() as i64,
    };
    an.marks = place_marks(&hp, &an, max_p, cfg.min_period());
    an
}

fn place_marks(x: &[f32], an: &Analysis, max_p: usize, min_p: usize) -> Vec<Mark> {
    let sr = an.sample_rate;
    let unvoiced = ((sr / 250.0).round() as usize).clamp(8, max_p);
    let at = |t: i64| {
        if t >= 0 && (t as usize) < x.len() {
            x[t as usize]
        } else {
            0.0
        }
    };
    let mut marks: Vec<Mark> = Vec::with_capacity(x.len() / min_p.max(1) + 2);
    let mut pos = 0i64;
    // One mark at or past the end, so the last grain's right half covers the tail.
    while marks.last().is_none_or(|m| m.pos < an.len) {
        let midi = an.midi_at(pos);
        let voiced = midi > 0.0;
        let spacing = if voiced {
            ((sr / crate::scale::midi_to_freq(midi)).round() as usize).clamp(min_p.max(2), max_p)
        } else {
            unvoiced
        };
        if let Some(last) = marks.last_mut()
            && last.voiced
            && voiced
        {
            pos = align_mark(at, last.pos, pos, spacing);
            last.spacing = (pos - last.pos).max(2) as u32;
        }
        marks.push(Mark {
            pos,
            spacing: spacing as u32,
            voiced,
        });
        pos += spacing as i64;
    }
    marks
}

/// Shortest note, seconds.
const MIN_NOTE_S: f32 = 0.06;

/// Splits the voiced stretches of a take into notes.
pub fn segment(an: &Analysis) -> Vec<Note> {
    let hop = an.hop as i64;
    let n = an.frames.len();
    let fps = an.sample_rate / an.hop as f32;
    let min_frames = (MIN_NOTE_S * fps).ceil() as usize;
    // Slow centre (zero-phase two-pass one-pole at ~4 Hz) for split decisions: vibrato out.
    let a = 1.0 - (-std::f32::consts::TAU * 4.0 / fps).exp();
    let mut notes = Vec::new();
    let mut i = 0;
    while i < n {
        if an.frames[i].midi <= 0.0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && an.frames[i].midi > 0.0 {
            i += 1;
        }
        let end = i;
        if end - start < min_frames {
            continue;
        }
        let m: Vec<f32> = an.frames[start..end].iter().map(|f| f.midi).collect();
        let mut slow = m.clone();
        let mut y = slow[0];
        for v in slow.iter_mut() {
            y += a * (*v - y);
            *v = y;
        }
        let mut y = slow[slow.len() - 1];
        for v in slow.iter_mut().rev() {
            y += a * (*v - y);
            *v = y;
        }
        // Walk the run: a split where the slow pitch leaves the current note's centre by
        // more than 0.7 st and stays away for 50 ms.
        let hold = ((0.05 * fps).ceil() as usize).max(1);
        let mut bounds = vec![0usize];
        let mut seg_start = 0;
        let mut j = 0;
        while j < slow.len() {
            let mut seg: Vec<f32> = slow[seg_start..=j].to_vec();
            let centre = median(&mut seg);
            if (slow[j] - centre).abs() > 0.7 && j + hold <= slow.len() {
                let away = slow[j..j + hold].iter().all(|v| (v - centre).abs() > 0.5);
                if away {
                    // Split where the slide crossed half-way.
                    let target = slow[(j + hold).min(slow.len() - 1)];
                    let mid = 0.5 * (centre + target);
                    let mut k = j;
                    while k > seg_start + 1
                        && (slow[k - 1] - mid).signum() == (slow[j] - mid).signum()
                    {
                        k -= 1;
                    }
                    if k - seg_start >= min_frames && slow.len() - k >= min_frames {
                        bounds.push(k);
                        seg_start = k;
                        j = k + hold;
                        continue;
                    }
                }
            }
            j += 1;
        }
        bounds.push(slow.len());
        // Short pieces of a slide aren't notes: merge any piece under 250 ms whose pitch
        // moves more than 0.6 st into the note the slide arrives at (or the one before).
        let short = (0.25 * fps) as usize;
        let mut k = 1;
        while bounds.len() > 2 && k < bounds.len() {
            let (a0, b0) = (bounds[k - 1], bounds[k]);
            let piece = &slow[a0..b0];
            let spread = piece.iter().copied().fold(f32::MIN, f32::max)
                - piece.iter().copied().fold(f32::MAX, f32::min);
            if b0 - a0 < short && spread > 0.6 {
                // Drop the boundary towards the arrival note.
                if k + 1 < bounds.len() {
                    bounds.remove(k);
                } else {
                    bounds.remove(k - 1);
                }
                k = 1;
                continue;
            }
            k += 1;
        }
        for w in bounds.windows(2) {
            let (a0, b0) = (w[0], w[1]);
            // Pitch centre: median of the middle 70% (onsets and releases slide).
            let len = b0 - a0;
            let trim = len * 15 / 100;
            let mut core: Vec<f32> = m[a0 + trim..b0 - trim].to_vec();
            let pitch = median(&mut core);
            let s = (start + a0) as i64 * hop - hop / 2;
            let e = (start + b0) as i64 * hop - hop / 2;
            let (s, e) = (s.max(0), e.min(an.len));
            notes.push(Note {
                src_start: s,
                src_end: e,
                dst_start: s,
                dst_end: e,
                pitch,
                shift: 0.0,
            });
        }
    }
    notes
}

/// A take's notes and their edits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteEdits {
    /// Notes, ascending and non-overlapping in both source and destination.
    pub notes: Vec<Note>,
    /// Take length, samples.
    pub len: i64,
}

/// Shortest a note may be stretched or split to, samples at 48 kHz scale: 40 ms.
fn min_len(sr: f32) -> i64 {
    (0.04 * sr) as i64
}

impl NoteEdits {
    /// Fresh, unedited notes for an analysed take.
    pub fn new(an: &Analysis) -> Self {
        Self {
            notes: segment(an),
            len: an.len,
        }
    }

    /// Whether anything differs from the take as sung.
    pub fn is_edited(&self) -> bool {
        self.notes
            .iter()
            .any(|n| n.shift != 0.0 || n.dst_start != n.src_start || n.dst_end != n.src_end)
    }

    fn anchors(&self) -> Vec<(i64, i64)> {
        let mut a = Vec::with_capacity(self.notes.len() * 2 + 2);
        a.push((0, 0));
        for n in &self.notes {
            for p in [(n.src_start, n.dst_start), (n.src_end, n.dst_end)] {
                match a.last() {
                    Some(l) if l.0 >= p.0 => {}
                    _ => a.push(p),
                }
            }
        }
        if a.last().is_some_and(|l| l.0 < self.len) {
            a.push((self.len, self.len));
        }
        a
    }

    /// Source time → destination time.
    pub fn map(&self, src: f64) -> f64 {
        interp(&self.anchors(), src, false)
    }

    /// Destination time → source time.
    pub fn unmap(&self, dst: f64) -> f64 {
        interp(&self.anchors(), dst, true)
    }

    /// Sets note `i`'s pitch edit (semitones).
    pub fn set_shift(&mut self, i: usize, shift: f32) {
        if let Some(n) = self.notes.get_mut(i) {
            n.shift = shift.clamp(-24.0, 24.0);
        }
    }

    fn contiguous(&self, i: usize) -> bool {
        match (self.notes.get(i), self.notes.get(i + 1)) {
            (Some(a), Some(b)) => a.src_end == b.src_start,
            _ => false,
        }
    }

    /// Moves note `i`'s end (destination time), squeezing or growing what follows.
    /// Returns the end actually set.
    pub fn set_end(&mut self, i: usize, dst: i64, sample_rate: f32) -> i64 {
        let ml = min_len(sample_rate);
        let Some(n) = self.notes.get(i).copied() else {
            return dst;
        };
        let src_len = n.src_end - n.src_start;
        // Never stretch past 4x or squeeze under 1/4.
        let mut lo = n.dst_start + ml.max(src_len / 4);
        let mut hi = n.dst_start + 4 * src_len;
        if self.contiguous(i) {
            let next = self.notes[i + 1];
            hi = hi.min(next.dst_end - ml.max((next.src_end - next.src_start) / 4));
        } else if let Some(next) = self.notes.get(i + 1) {
            hi = hi.min(next.dst_start);
        } else {
            hi = hi.min(self.len);
        }
        lo = lo.min(hi);
        let v = dst.clamp(lo, hi);
        self.notes[i].dst_end = v;
        if self.contiguous(i) {
            self.notes[i + 1].dst_start = v;
        }
        v
    }

    /// Moves note `i`'s start (destination time). Returns the start actually set.
    pub fn set_start(&mut self, i: usize, dst: i64, sample_rate: f32) -> i64 {
        let ml = min_len(sample_rate);
        let Some(n) = self.notes.get(i).copied() else {
            return dst;
        };
        let src_len = n.src_end - n.src_start;
        let mut hi = n.dst_end - ml.max(src_len / 4);
        let mut lo = n.dst_end - 4 * src_len;
        if i > 0 && self.contiguous(i - 1) {
            let prev = self.notes[i - 1];
            lo = lo.max(prev.dst_start + ml.max((prev.src_end - prev.src_start) / 4));
        } else if i > 0 {
            lo = lo.max(self.notes[i - 1].dst_end);
        } else {
            lo = lo.max(0);
        }
        hi = hi.max(lo);
        let v = dst.clamp(lo, hi);
        self.notes[i].dst_start = v;
        if i > 0 && self.contiguous(i - 1) {
            self.notes[i - 1].dst_end = v;
        }
        v
    }

    /// Splits note `i` at destination time `dst`. Both halves keep the note's edit and
    /// get their own sung pitch centre. Returns false if too close to an end.
    pub fn split(&mut self, i: usize, dst: i64, an: &Analysis) -> bool {
        let ml = min_len(an.sample_rate);
        let Some(n) = self.notes.get(i).copied() else {
            return false;
        };
        if dst - n.dst_start < ml || n.dst_end - dst < ml {
            return false;
        }
        let src = self.unmap(dst as f64).round() as i64;
        if src - n.src_start < ml / 2 || n.src_end - src < ml / 2 {
            return false;
        }
        let mut a = n;
        let mut b = n;
        a.src_end = src;
        a.dst_end = dst;
        b.src_start = src;
        b.dst_start = dst;
        a.pitch = pitch_of(an, a.src_start, a.src_end).unwrap_or(n.pitch);
        b.pitch = pitch_of(an, b.src_start, b.src_end).unwrap_or(n.pitch);
        self.notes[i] = a;
        self.notes.insert(i + 1, b);
        true
    }

    /// Moves every note's centre onto the nearest note of `scale`.
    pub fn snap_all(&mut self, scale: &Scale) {
        for n in self.notes.iter_mut() {
            n.shift = scale.nearest(n.target()) as f32 - n.pitch;
        }
    }

    /// Clears every edit.
    pub fn reset(&mut self) {
        for n in self.notes.iter_mut() {
            n.shift = 0.0;
            n.dst_start = n.src_start;
            n.dst_end = n.src_end;
        }
    }

    /// Index of the note playing at destination time `dst`.
    pub fn note_at(&self, dst: i64) -> Option<usize> {
        self.notes
            .iter()
            .position(|n| dst >= n.dst_start && dst < n.dst_end)
    }

    /// The pitch shift (semitones) to apply at source time `src`, smoothed over ~25 ms at
    /// boundaries between notes with different shifts so edits never jump.
    pub fn shift_at(&self, src: f64, sample_rate: f32) -> f32 {
        let glide = (0.0125 * sample_rate) as f64;
        let raw = |t: f64| -> f32 {
            self.notes
                .iter()
                .find(|n| t >= n.src_start as f64 && t < n.src_end as f64)
                .map_or(0.0, |n| n.shift)
        };
        // Average over a ±glide window, sampled: a raised ramp at each boundary.
        const TAPS: usize = 9;
        let mut sum = 0.0;
        for k in 0..TAPS {
            let off = (k as f64 / (TAPS - 1) as f64 * 2.0 - 1.0) * glide;
            sum += raw(src + off);
        }
        sum / TAPS as f32
    }
}

fn pitch_of(an: &Analysis, s: i64, e: i64) -> Option<f32> {
    let hop = an.hop as i64;
    let (a, b) = (
        (s / hop) as usize,
        ((e / hop) as usize).min(an.frames.len()),
    );
    let mut v: Vec<f32> = an
        .frames
        .get(a..b)?
        .iter()
        .map(|f| f.midi)
        .filter(|m| *m > 0.0)
        .collect();
    let trim = v.len() * 15 / 100;
    if v.len() < 3 {
        return None;
    }
    let len = v.len();
    let mut core = v.drain(trim..len - trim).collect::<Vec<_>>();
    Some(median(&mut core))
}

fn interp(anchors: &[(i64, i64)], t: f64, inverse: bool) -> f64 {
    let key = |p: &(i64, i64)| if inverse { p.1 } else { p.0 } as f64;
    let val = |p: &(i64, i64)| if inverse { p.0 } else { p.1 } as f64;
    let i = anchors.partition_point(|p| key(p) <= t);
    if i == 0 {
        return t;
    }
    if i >= anchors.len() {
        let l = &anchors[anchors.len() - 1];
        return val(l) + (t - key(l));
    }
    let (a, b) = (&anchors[i - 1], &anchors[i]);
    let span = key(b) - key(a);
    if span <= 0.0 {
        return val(a);
    }
    val(a) + (t - key(a)) / span * (val(b) - val(a))
}

/// Formant handling for rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Formant {
    /// Keep formants in place.
    pub preserve: bool,
    /// Extra formant shift, semitones.
    pub shift: f32,
}

/// Renders the edited take. `channels` are the take's channels (equal length). The
/// result has the same length; regions no edit touches come out identical to the input.
pub fn render(
    channels: &[&[f32]],
    an: &Analysis,
    edits: &NoteEdits,
    formant: Formant,
) -> Vec<Vec<f32>> {
    let len = an.len.max(0) as usize;
    let nch = channels.len();
    let mut out = vec![vec![0f32; len]; nch];
    if len == 0 || an.marks.is_empty() {
        for (o, c) in out.iter_mut().zip(channels) {
            o.copy_from_slice(&c[..len.min(c.len())]);
        }
        return out;
    }
    // Source rings: the take padded so grains may reach past the ends. Power-of-two sized
    // so the shared OLA code's masks work; the accumulators likewise.
    let size = (len + 8192).next_power_of_two();
    let mask = size - 1;
    let src: Vec<Vec<f32>> = channels
        .iter()
        .map(|c| {
            let mut v = vec![0f32; size];
            v[..len].copy_from_slice(&c[..len]);
            v
        })
        .collect();
    let mut acc = vec![vec![0f32; size]; nch];
    let mut wsum = vec![0f32; size];
    let sr = an.sample_rate;
    let rate_formant = (formant.shift.clamp(-12.0, 12.0) / 12.0).exp2();
    let unvoiced_step = an
        .marks
        .iter()
        .find(|m| !m.voiced)
        .map_or(176, |m| m.spacing) as i64;
    let nearest = |t: f64| -> usize {
        let i = an.marks.partition_point(|m| (m.pos as f64) <= t);
        if i == 0 {
            0
        } else if i >= an.marks.len() {
            an.marks.len() - 1
        } else if (an.marks[i].pos as f64 - t) < (t - an.marks[i - 1].pos as f64) {
            i
        } else {
            i - 1
        }
    };
    let mut o: i64 = 0;
    let mut last_o: i64 = -1;
    let mut synced = true;
    while (o as usize) < len + 64 {
        let tau = edits.unmap(o as f64);
        let mut k = nearest(tau);
        if synced {
            // Snap onto the mark at or after tau, placed at its mapped time.
            while k + 1 < an.marks.len() && (an.marks[k].pos as f64) < tau - 0.5 {
                k += 1;
            }
            let mapped = edits.map(an.marks[k].pos as f64).round() as i64;
            if mapped > o {
                o = mapped;
            }
        }
        let m = an.marks[k];
        let prev_spacing = if k > 0 {
            (m.pos - an.marks[k - 1].pos).max(1) as f32
        } else {
            m.spacing as f32
        };
        let ratio = if m.voiced {
            (edits.shift_at(tau, sr) / 12.0).exp2()
        } else {
            1.0
        };
        let (rate, step) = if m.voiced {
            let rate = if formant.preserve {
                rate_formant
            } else {
                ratio
            };
            (rate, (m.spacing as f32 / ratio).round().max(2.0) as i64)
        } else {
            (1.0, m.spacing as i64)
        };
        let left = (prev_spacing / rate)
            .min((o - last_o).max(1) as f32)
            .round()
            .max(1.0) as usize;
        let right = (m.spacing as f32 / rate).min(step as f32).round().max(1.0) as usize;
        let g = Grain {
            out: o,
            src: m.pos as f64,
            left,
            right,
            rate,
        };
        for c in 0..nch {
            let ws = if c == 0 {
                Some(wsum.as_mut_slice())
            } else {
                None
            };
            add_grain(&g, &src[c], mask, &mut acc[c], mask, ws);
        }
        last_o = o;
        if m.voiced {
            synced = false;
            o += step;
        } else {
            // Unvoiced: where the map is locally 1:1 the next grain lands exactly on its
            // own mark (an exact copy); stretched, grains repeat or skip at a fixed step.
            let next = an
                .marks
                .get(k + 1)
                .map(|n| edits.map(n.pos as f64).round() as i64);
            match next {
                Some(nx) if nx > o + unvoiced_step / 2 && nx < o + 2 * unvoiced_step => {
                    o = nx;
                    synced = true;
                }
                _ => {
                    o += unvoiced_step.max(1);
                    synced = false;
                }
            }
        }
    }
    for c in 0..nch {
        for t in 0..len {
            out[c][t] = normalise(acc[c][t], wsum[t]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edits(notes: &[(i64, i64)]) -> NoteEdits {
        NoteEdits {
            notes: notes
                .iter()
                .map(|&(s, e)| Note {
                    src_start: s,
                    src_end: e,
                    dst_start: s,
                    dst_end: e,
                    pitch: 60.0,
                    shift: 0.0,
                })
                .collect(),
            len: 48_000,
        }
    }

    #[test]
    fn stretching_squeezes_the_neighbour() {
        let mut e = edits(&[(4800, 14400), (14400, 24000), (30000, 40000)]);
        let set = e.set_end(0, 19200, 48_000.0);
        assert_eq!(set, 19200);
        assert_eq!(e.notes[1].dst_start, 19200);
        assert_eq!(e.notes[1].dst_end, 24000);
        // Times inside the second note now map into its squeezed span.
        assert!((e.map(14400.0) - 19200.0).abs() < 1e-6);
        assert!((e.unmap(19200.0) - 14400.0).abs() < 1e-6);
        // Past the edit nothing moved.
        assert!((e.map(35000.0) - 35000.0).abs() < 1e-6);
        // Not contiguous: the second note's end can't pass the third note's start.
        assert_eq!(e.set_end(1, 33000, 48_000.0), 30000);
    }

    #[test]
    fn split_makes_two_contiguous_notes() {
        let mut e = edits(&[(4800, 24000)]);
        let an = Analysis {
            sample_rate: 48_000.0,
            hop: 128,
            frames: vec![
                TrackFrame {
                    midi: 60.0,
                    rms: 0.1
                };
                400
            ],
            marks: Vec::new(),
            len: 48_000,
        };
        assert!(e.split(0, 12000, &an));
        assert_eq!(e.notes.len(), 2);
        assert_eq!(e.notes[0].src_end, e.notes[1].src_start);
        assert!(!e.split(0, 4900, &an));
    }
}
