//! The output stage every style shares: Gain In, the parallel Wet/Dry/Mix blend with the
//! dry path delayed to match, auto-makeup and Gain Out. Same behaviour as the compressor's
//! and the EQ's.

use solplugs_comp_core::AvgPeak;
use solplugs_comp_core::util::{Delay, Smooth, db_to_lin};

use crate::VersionSettings as Common;

/// Parameter smoothing, ms.
const SMOOTH_MS: f32 = 20.0;
/// Auto-makeup: fraction of the gap closed per counted 20 ms window (about 1 s).
const AUTO_SMOOTH: f32 = 0.02;

/// Gains for one sample.
#[derive(Clone, Copy, Debug)]
pub struct Gains {
    /// Gain In, linear.
    pub gin: f32,
    /// Gain Out, linear.
    pub gout: f32,
    /// Wet share (mix * wet level).
    pub wet: f32,
    /// Dry share ((1 - mix) * dry level).
    pub dry: f32,
}

/// What a chain measured in one call.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChainStats {
    /// Peak of the wet part of the output, linear.
    pub wet_peak: f32,
    /// Peak of the dry part of the output, linear.
    pub dry_peak: f32,
}

/// Shared output stage.
#[derive(Clone, Debug)]
pub struct Mixer {
    gin: Smooth,
    gout: Smooth,
    mix: Smooth,
    wet: Smooth,
    dry: Smooth,
    dry_delay: [Delay; 2],
    before: AvgPeak,
    after: AvgPeak,
    auto_db: f32,
    sr: f32,
}

/// Wet/dry level in dB; -60 and below is off.
#[inline]
fn level(db: f32) -> f32 {
    if db <= -59.9 { 0.0 } else { db_to_lin(db) }
}

impl Mixer {
    /// A mixer at `sr` whose dry path can be delayed up to `max_delay` samples.
    pub fn new(sr: f32, max_delay: usize) -> Self {
        let s = Smooth::new(SMOOTH_MS, sr);
        Self {
            gin: s,
            gout: s,
            mix: s,
            wet: s,
            dry: s,
            dry_delay: [Delay::new(max_delay), Delay::new(max_delay)],
            before: AvgPeak::new(sr),
            after: AvgPeak::new(sr),
            auto_db: 0.0,
            sr,
        }
    }

    /// Clears all history without allocating.
    pub fn reset(&mut self) {
        for d in &mut self.dry_delay {
            d.clear();
        }
        self.before.reset();
        self.after.reset();
        self.auto_db = 0.0;
        let s = Smooth::new(SMOOTH_MS, self.sr);
        self.gin = s;
        self.gout = s;
        self.mix = s;
        self.wet = s;
        self.dry = s;
    }

    /// Sets the dry path's delay (the chain's latency).
    pub fn set_latency(&mut self, n: usize) {
        for d in &mut self.dry_delay {
            d.set_len(n);
        }
    }

    /// Gain the auto-makeup measures, dB.
    pub fn auto_db(&self) -> f32 {
        self.auto_db
    }

    /// Next sample's gains.
    #[inline]
    pub fn step(&mut self, c: &Common) -> Gains {
        let gout_target = if c.auto_makeup {
            self.auto_db
        } else {
            c.gain_out_db
        };
        let gin = db_to_lin(self.gin.next(c.gain_in_db));
        let gout = db_to_lin(self.gout.next(gout_target));
        let mix = self.mix.next(c.mix.clamp(0.0, 1.0));
        let wet = level(self.wet.next(c.wet_db));
        let dry = level(self.dry.next(c.dry_db));
        Gains {
            gin,
            gout,
            wet: mix * wet,
            dry: (1.0 - mix) * dry,
        }
    }

    /// Blends channel `ch`: `x` is the raw input, `eq` the wet signal (Gain In included).
    /// Returns the output before Gain Out and the final output.
    #[inline]
    pub fn blend(
        &mut self,
        ch: usize,
        x: f32,
        eq: f32,
        g: &Gains,
        st: &mut ChainStats,
    ) -> (f32, f32) {
        let d = self.dry_delay[ch].process(x) * g.dry;
        let w = eq * g.wet;
        st.wet_peak = st.wet_peak.max((w * g.gout).abs());
        st.dry_peak = st.dry_peak.max((d * g.gout).abs());
        let y = w + d;
        (y, y * g.gout)
    }

    /// Auto-makeup: matches the long-term peak of the output (before Gain Out) to the
    /// input. Silence holds the last value.
    #[inline]
    pub fn track(&mut self, before: f32, after: f32) {
        let gate = self.before.push(before);
        self.after.push_gated(after, gate);
        if gate {
            let a = self.before.db();
            let b = self.after.db();
            if a > -90.0 && b > -90.0 {
                let target = (a - b).clamp(-24.0, 24.0);
                self.auto_db += (target - self.auto_db) * AUTO_SMOOTH;
            }
        }
    }
}
