//! The real-time pitch corrector: detection, correction and PSOLA shifting on a live
//! stream, at a fixed latency.
//!
//! Timeline (absolute sample indices, `now` = samples received so far):
//! - every hop, the McLeod detector analyses the last `2·P_max` samples; the frame
//!   describes the window centre, `now - P_max`;
//! - analysis marks are laid one local period apart (a fixed short spacing when unvoiced)
//!   as soon as the source data for their grain is in;
//! - output marks are laid one *corrected* period apart; each takes the grain of the
//!   nearest analysis mark;
//! - output is emitted at `now - latency`, with `latency = 2·P_max + 2·hop`: a grain is two
//!   periods long, and the marks it is built from must already have their frame.
//!
//! Unvoiced stretches (breaths, consonants, silence) are copied grain-for-grain at their
//! own positions, so they come out exactly as they went in.

use crate::correct::{CorrectParams, Corrected, Corrector};
use crate::detect::{Detection, Mpm};
use crate::ola::{Grain, add_grain, align_mark, normalise};
use crate::scale::freq_to_midi;

/// Maximum channels processed (shared marks, per-channel grains).
pub const MAX_CHANNELS: usize = 2;
const RING: usize = 1 << 16;
const MASK: usize = RING - 1;
const FRAMES: usize = 512;
const MARKS: usize = 1024;

/// Singer range: sets the lowest detectable pitch and therefore the latency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Range {
    /// Soprano, alto, children, most lead instruments: 100 Hz (G2) and up.
    High,
    /// Tenor, alto, baritone: 75 Hz (D2) and up.
    #[default]
    Mid,
    /// Bass, low baritone: 55 Hz (A1) and up.
    Low,
}

impl Range {
    /// All ranges, UI order.
    pub const ALL: [Range; 3] = [Range::High, Range::Mid, Range::Low];

    /// Lowest pitch, Hz.
    pub fn min_hz(self) -> f32 {
        match self {
            Range::High => 100.0,
            Range::Mid => 75.0,
            Range::Low => 55.0,
        }
    }

    /// Highest pitch, Hz.
    pub fn max_hz(self) -> f32 {
        match self {
            Range::High => 1400.0,
            Range::Mid => 1000.0,
            Range::Low => 700.0,
        }
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Range::High => "High voice",
            Range::Mid => "Mid voice",
            Range::Low => "Low voice",
        }
    }

    /// Index in [`Range::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|r| *r == self).unwrap_or(1)
    }

    /// From an index, clamped.
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(2)]
    }
}

/// Fixed configuration (changing it means a new [`Realtime`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Sample rate, Hz.
    pub sample_rate: f32,
    /// Singer range.
    pub range: Range,
}

impl Config {
    /// Longest period, samples.
    pub fn max_period(&self) -> usize {
        (self.sample_rate / self.range.min_hz()).ceil() as usize
    }

    /// Shortest period, samples.
    pub fn min_period(&self) -> usize {
        (self.sample_rate / self.range.max_hz()).floor().max(2.0) as usize
    }

    /// Analysis hop, samples (~2.7 ms).
    pub fn hop(&self) -> usize {
        ((self.sample_rate / 375.0).round() as usize).clamp(16, 512)
    }

    /// Detection window, samples.
    pub fn window(&self) -> usize {
        2 * self.max_period()
    }

    /// Processing latency, samples.
    pub fn latency(&self) -> usize {
        2 * self.max_period() + 2 * self.hop()
    }

    /// Mark spacing in unvoiced stretches, samples (~4 ms).
    fn unvoiced_spacing(&self) -> usize {
        ((self.sample_rate / 250.0).round() as usize).clamp(8, self.max_period())
    }
}

/// Live controls, read every block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Correction settings.
    pub correct: CorrectParams,
    /// Keep the formants in place while shifting (on by default).
    pub formant_preserve: bool,
    /// Extra formant shift in semitones (only with formant preservation on).
    pub formant_shift: f32,
    /// Bypass: output is the input delayed by the latency, bit-identical.
    pub bypass: bool,
    /// Fixed transpose on top of the correction, semitones.
    pub transpose: f32,
    /// Signal level (RMS, linear) below which a frame counts as silence.
    pub gate: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            correct: CorrectParams::default(),
            formant_preserve: true,
            formant_shift: 0.0,
            bypass: false,
            transpose: 0.0,
            gate: 10f32.powf(-55.0 / 20.0),
        }
    }
}

/// What one analysis frame found, for meters and displays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameInfo {
    /// Absolute sample time of the frame (window centre).
    pub time: i64,
    /// Correction result (input/output pitch, note, shift).
    pub corrected: Corrected,
    /// Detector clarity, 0..1.
    pub clarity: f32,
    /// Window RMS.
    pub rms: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    centre: i64,
    /// Period in samples (median-filtered), 0 when unvoiced.
    period: f32,
    /// Pitch ratio to apply.
    ratio: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Mark {
    pos: i64,
    spacing: usize,
    voiced: bool,
}

/// The real-time corrector for up to [`MAX_CHANNELS`] channels.
pub struct Realtime {
    cfg: Config,
    hop: usize,
    latency: i64,
    mpm: Mpm,
    corrector: Corrector,
    // Input.
    input: [Vec<f32>; MAX_CHANNELS],
    mono: Vec<f32>,
    window_buf: Vec<f32>,
    hp_x: f32,
    hp_y: f32,
    hp_a: f32,
    now: i64,
    next_frame_end: i64,
    // Analysis.
    raw: [Detection; 3],
    raw_voiced: [bool; 3],
    voiced: bool,
    frames: Vec<Frame>,
    frame_count: i64,
    marks: Vec<Mark>,
    mark_count: i64,
    // Synthesis.
    acc: [Vec<f32>; MAX_CHANNELS],
    wsum: Vec<f32>,
    next_out: i64,
    last_out: i64,
    synced: bool,
    emitted: i64,
    // Bypass crossfade, 0 = dry, 1 = wet.
    wet: f32,
    fade_step: f32,
}

impl Realtime {
    /// A new processor.
    pub fn new(cfg: Config) -> Self {
        let hop = cfg.hop();
        let win = cfg.window();
        let latency = cfg.latency() as i64;
        Self {
            cfg,
            hop,
            latency,
            mpm: Mpm::new(win, cfg.min_period(), cfg.max_period()),
            corrector: Corrector::new(hop as f32 * 1000.0 / cfg.sample_rate),
            input: std::array::from_fn(|_| vec![0.0; RING]),
            mono: vec![0.0; RING],
            window_buf: vec![0.0; win],
            hp_x: 0.0,
            hp_y: 0.0,
            hp_a: (-std::f32::consts::TAU * 50.0 / cfg.sample_rate).exp(),
            now: 0,
            next_frame_end: win as i64,
            raw: [Detection::default(); 3],
            raw_voiced: [false; 3],
            voiced: false,
            frames: vec![Frame::default(); FRAMES],
            frame_count: 0,
            marks: vec![Mark::default(); MARKS],
            mark_count: 0,
            acc: std::array::from_fn(|_| vec![0.0; RING]),
            wsum: vec![0.0; RING],
            next_out: 0,
            last_out: -1,
            synced: true,
            emitted: -latency,
            wet: 1.0,
            fade_step: 1.0 / (0.01 * cfg.sample_rate).max(1.0),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Latency in samples.
    pub fn latency(&self) -> usize {
        self.latency as usize
    }

    /// Clears all state (silence in, silence out), keeping the configuration.
    pub fn reset(&mut self) {
        let cfg = self.cfg;
        let wet = self.wet;
        *self = Self::new(cfg);
        self.wet = wet;
    }

    /// Processes `channels` in place (1 or 2 used; equal lengths). `on_frame` receives each
    /// analysis frame. Never allocates.
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        params: &Params,
        on_frame: &mut impl FnMut(&FrameInfo),
    ) {
        let nch = channels.len().min(MAX_CHANNELS);
        if nch == 0 {
            return;
        }
        let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let mut done = 0;
        while done < n {
            let k = (n - done).min(self.hop);
            self.push_input(channels, nch, done, k);
            while self.now >= self.next_frame_end {
                self.analyse(params, on_frame);
            }
            self.extend_marks();
            self.synthesise(nch, params);
            self.emit(channels, nch, done, k, params);
            done += k;
        }
    }

    fn push_input(&mut self, channels: &[&mut [f32]], nch: usize, off: usize, k: usize) {
        let inv = 1.0 / nch as f32;
        for i in 0..k {
            let t = (self.now as usize + i) & MASK;
            let mut sum = 0.0;
            for c in 0..nch {
                let x = channels[c][off + i];
                self.input[c][t] = x;
                sum += x;
            }
            // Analysis signal: mono, DC and rumble removed (one-pole high-pass at 50 Hz).
            let x = sum * inv;
            let y = self.hp_a * (self.hp_y + x - self.hp_x);
            self.hp_x = x;
            self.hp_y = y;
            self.mono[t] = y;
        }
        self.now += k as i64;
    }

    fn analyse(&mut self, params: &Params, on_frame: &mut impl FnMut(&FrameInfo)) {
        let end = self.next_frame_end;
        self.next_frame_end += self.hop as i64;
        let win = self.window_buf.len();
        let start = end - win as i64;
        for (i, w) in self.window_buf.iter_mut().enumerate() {
            *w = self.mono[((start + i as i64) as usize) & MASK];
        }
        let hint = self.raw_voiced[2].then_some(self.raw[2].period);
        let d = self.mpm.detect_near(&self.window_buf, hint);
        // Voicing with hysteresis on clarity, gated on level.
        let thr = if self.raw_voiced[2] { 0.6 } else { 0.8 };
        let v = d.period > 0.0 && d.clarity >= thr && d.rms >= params.gate;
        self.raw.rotate_left(1);
        self.raw_voiced.rotate_left(1);
        self.raw[2] = d;
        self.raw_voiced[2] = v;
        // Median of three removes single-frame octave slips; the result describes the
        // middle frame.
        let centre = end - (win / 2) as i64 - self.hop as i64;
        let voiced = self.raw_voiced[1] && (self.raw_voiced[0] || self.raw_voiced[2]);
        let period = if voiced {
            let mut p = [0f32; 3];
            let mut np = 0;
            for j in 0..3 {
                if self.raw_voiced[j] {
                    p[np] = self.raw[j].period;
                    np += 1;
                }
            }
            if np == 3 {
                p.sort_by(f32::total_cmp);
                p[1]
            } else {
                self.raw[1].period
            }
        } else {
            0.0
        };
        self.voiced = voiced;
        let midi = (period > 0.0).then(|| freq_to_midi(self.cfg.sample_rate / period));
        let corrected = self.corrector.step(midi, &params.correct);
        let ratio = if voiced {
            ((corrected.shift + params.transpose).clamp(-24.0, 24.0) / 12.0).exp2()
        } else {
            1.0
        };
        let idx = (self.frame_count as usize) % FRAMES;
        self.frames[idx] = Frame {
            centre,
            period,
            ratio,
        };
        self.frame_count += 1;
        on_frame(&FrameInfo {
            time: centre,
            corrected,
            clarity: self.raw[1].clarity,
            rms: self.raw[1].rms,
        });
    }

    /// The frame (period, ratio) at absolute time `t`, linearly interpolated between voiced
    /// frames, held at the ends.
    fn frame_at(&self, t: i64) -> (f32, f32) {
        if self.frame_count == 0 {
            return (0.0, 1.0);
        }
        let first = (self.frame_count - FRAMES as i64).max(0);
        let last = self.frame_count - 1;
        let get = |k: i64| self.frames[(k as usize) % FRAMES];
        let c0 = get(first).centre;
        let pos = (t - c0) as f64 / self.hop as f64;
        let k = (first + pos.floor().max(0.0) as i64).min(last);
        let a = get(k);
        if k == last || t <= a.centre {
            return (a.period, a.ratio);
        }
        let b = get(k + 1);
        let f = ((t - a.centre) as f32 / self.hop as f32).clamp(0.0, 1.0);
        match (a.period > 0.0, b.period > 0.0) {
            (true, true) => (
                a.period + f * (b.period - a.period),
                a.ratio + f * (b.ratio - a.ratio),
            ),
            _ if f < 0.5 => (a.period, a.ratio),
            _ => (b.period, b.ratio),
        }
    }

    fn latest_centre(&self) -> i64 {
        if self.frame_count == 0 {
            return i64::MIN / 2;
        }
        self.frames[((self.frame_count - 1) as usize) % FRAMES].centre
    }

    fn mark(&self, k: i64) -> Mark {
        self.marks[(k as usize) % MARKS]
    }

    fn extend_marks(&mut self) {
        let unvoiced = self.cfg.unvoiced_spacing();
        loop {
            let prev = (self.mark_count > 0).then(|| self.mark(self.mark_count - 1));
            let mut pos = match prev {
                // First mark: where output starts.
                None => self.emitted.max(0),
                Some(m) => m.pos + m.spacing as i64,
            };
            let (period, _) = self.frame_at(pos);
            let voiced = period > 0.0;
            let spacing = if voiced {
                (period.round() as usize).clamp(2, self.cfg.max_period())
            } else {
                unvoiced
            };
            // The grain's source must be in, and its voicing must come from a real frame
            // (a frame describes ±P_max around its centre; half of that is trusted).
            let reach = spacing as i64 + (spacing / 5) as i64 + 1;
            let horizon = self.latest_centre() + (self.cfg.max_period() / 2) as i64;
            if pos + reach > self.now || (self.frame_count > 0 && pos > horizon) {
                break;
            }
            if let Some(m) = prev.filter(|m| m.voiced && voiced) {
                // Snap onto the waveform: the next period starts where the signal best
                // repeats the last one, so neighbouring grains line up even mid-slide.
                pos = self.align(m.pos, pos, spacing);
                if let Some(slot) = self.marks.get_mut(((self.mark_count - 1) as usize) % MARKS) {
                    slot.spacing = (pos - m.pos).max(2) as usize;
                }
            }
            let idx = (self.mark_count as usize) % MARKS;
            self.marks[idx] = Mark {
                pos,
                spacing,
                voiced,
            };
            self.mark_count += 1;
        }
    }

    fn align(&self, prev: i64, guess: i64, spacing: usize) -> i64 {
        align_mark(|t| self.mono[(t as usize) & MASK], prev, guess, spacing)
    }

    /// Index of the analysis mark nearest to `t` (searching back from the newest).
    fn nearest_mark(&self, t: i64) -> Option<i64> {
        if self.mark_count == 0 {
            return None;
        }
        let oldest = (self.mark_count - MARKS as i64 + 1).max(0);
        let mut k = self.mark_count - 1;
        while k > oldest && self.mark(k).pos > t {
            k -= 1;
        }
        let a = self.mark(k);
        if k + 1 < self.mark_count {
            let b = self.mark(k + 1);
            if (b.pos - t).abs() < (t - a.pos).abs() {
                return Some(k + 1);
            }
        }
        Some(k)
    }

    fn synthesise(&mut self, nch: usize, params: &Params) {
        let t_end = self.now - self.latency;
        loop {
            if self.mark_count == 0 {
                return;
            }
            if self.synced || self.next_out < self.mark(0).pos {
                // Resync onto the analysis mark at or after the output position.
                let Some(k) = self.nearest_mark(self.next_out) else {
                    return;
                };
                let mut k = k;
                while k + 1 < self.mark_count && self.mark(k).pos < self.next_out {
                    k += 1;
                }
                if self.mark(k).pos < self.next_out {
                    return;
                }
                self.next_out = self.mark(k).pos;
            }
            let o = self.next_out;
            let Some(k) = self.nearest_mark(o) else {
                return;
            };
            let m = self.mark(k);
            let prev_spacing = if k > 0 && k + MARKS as i64 > self.mark_count {
                (m.pos - self.mark(k - 1).pos).max(1) as usize
            } else {
                m.spacing
            };
            let (_, ratio) = self.frame_at(o);
            let (rate, step) = if m.voiced {
                let rate = if params.formant_preserve {
                    (params.formant_shift.clamp(-12.0, 12.0) / 12.0).exp2()
                } else {
                    ratio
                };
                let step = (m.spacing as f32 / ratio).round().max(2.0) as i64;
                (rate, step)
            } else {
                (1.0, m.spacing as i64)
            };
            // Each half reaches the neighbouring output mark, but never reads past the
            // neighbouring analysis mark.
            let out_prev = (o - self.last_out).clamp(1, 4 * self.cfg.max_period() as i64);
            let left = (prev_spacing as f32 / rate)
                .min(out_prev as f32)
                .round()
                .max(1.0) as usize;
            let right = (m.spacing as f32 / rate).min(step as f32).round().max(1.0) as usize;
            if o - left as i64 >= t_end {
                return;
            }
            let grain = Grain {
                out: o,
                src: m.pos as f64,
                left,
                right,
                rate,
            };
            for c in 0..nch {
                let ws = if c == 0 {
                    Some(self.wsum.as_mut_slice())
                } else {
                    None
                };
                add_grain(&grain, &self.input[c], MASK, &mut self.acc[c], MASK, ws);
            }
            self.last_out = o;
            if m.voiced {
                self.synced = false;
                self.next_out = o + step;
            } else {
                // Unvoiced: copy grains in place; the next output mark is the next
                // analysis mark.
                self.synced = true;
                self.next_out = m.pos + m.spacing as i64;
            }
        }
    }

    fn emit(
        &mut self,
        channels: &mut [&mut [f32]],
        nch: usize,
        off: usize,
        k: usize,
        params: &Params,
    ) {
        let start = self.now - k as i64 - self.latency;
        let target = if params.bypass { 0.0 } else { 1.0 };
        for i in 0..k {
            let t = start + i as i64;
            let idx = (t as usize) & MASK;
            if self.wet != target {
                self.wet = if self.wet < target {
                    (self.wet + self.fade_step).min(target)
                } else {
                    (self.wet - self.fade_step).max(target)
                };
            }
            let ws = self.wsum[idx];
            for c in 0..nch {
                let dry = if t >= 0 { self.input[c][idx] } else { 0.0 };
                let wet = normalise(self.acc[c][idx], ws);
                self.acc[c][idx] = 0.0;
                channels[c][off + i] = if self.wet <= 0.0 {
                    dry
                } else if self.wet >= 1.0 {
                    wet
                } else {
                    dry + self.wet * (wet - dry)
                };
            }
            self.wsum[idx] = 0.0;
        }
        self.emitted = start + k as i64;
    }
}
