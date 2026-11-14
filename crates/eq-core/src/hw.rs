//! A hardware style's chain: its bands and colour stage run 4x oversampled (so curves near
//! the top of the audio band keep their analogue shape, a 40 kHz Air Band shelf is real,
//! and the saturation doesn't alias), then a DC blocker and the shared output stage.

use solplugs_comp_core::chain::saturate;
use solplugs_comp_core::oversample::Oversampler;
use solplugs_comp_core::util::{DcBlock, Smooth};

use crate::filter::{BAND_FADE_MS, BAND_SMOOTH_MS, Band, BandSpec};
use crate::mixer::{ChainStats, Mixer};
use crate::style::{MAX_HW_BANDS, Style, hardware_bands};
use crate::{CONTROL, StyleSettings};

/// Oversampling factor.
pub const OS: usize = 4;

/// One hardware style.
pub struct HwChain {
    style: Style,
    sr: f32,
    bands: [Band; MAX_HW_BANDS],
    os: [Oversampler; 2],
    dc: [DcBlock; 2],
    colour: Smooth,
    mixer: Mixer,
    /// Samples until the next control update.
    countdown: usize,
}

impl HwChain {
    /// The chain for `style` at `sr`.
    pub fn new(style: Style, sr: f32) -> Self {
        let mut mixer = Mixer::new(sr, Oversampler::LATENCY + 1);
        mixer.set_latency(Oversampler::LATENCY);
        Self {
            style,
            sr,
            bands: Default::default(),
            os: [Oversampler::new(), Oversampler::new()],
            dc: [DcBlock::new(sr), DcBlock::new(sr)],
            colour: Smooth::new(20.0, sr),
            mixer,
            countdown: 0,
        }
    }

    /// Latency, samples.
    pub fn latency(&self) -> usize {
        Oversampler::LATENCY
    }

    /// Auto-makeup gain, dB.
    pub fn auto_db(&self) -> f32 {
        self.mixer.auto_db()
    }

    /// Clears all state.
    /// Doesn't allocate (it runs on the audio thread when the style changes).
    pub fn reset(&mut self) {
        for b in &mut self.bands {
            b.reset();
        }
        for o in &mut self.os {
            o.clear();
        }
        self.dc = [DcBlock::new(self.sr), DcBlock::new(self.sr)];
        self.colour = Smooth::new(20.0, self.sr);
        self.mixer.reset();
        self.countdown = 0;
    }

    /// The bands as the chain designs them now (for tests).
    pub fn target_bands(&self, s: &StyleSettings) -> [BandSpec; MAX_HW_BANDS] {
        hardware_bands(self.style, &s.knobs)
    }

    fn control(&mut self, s: &StyleSettings) {
        let fs = f64::from(self.sr) * OS as f64;
        let n = CONTROL as f64;
        let a = 1.0 - (-n / (BAND_SMOOTH_MS * 0.001 * f64::from(self.sr))).exp();
        let fade = n / (BAND_FADE_MS * 0.001 * f64::from(self.sr));
        let targets = hardware_bands(self.style, &s.knobs);
        for (b, t) in self.bands.iter_mut().zip(&targets) {
            b.update(t, [0.0; 2], fs, a, fade);
        }
    }

    /// Processes `input` into `out` (1 or 2 channels).
    pub fn process(
        &mut self,
        input: [&[f32]; 2],
        out: [&mut [f32]; 2],
        channels: usize,
        s: &StyleSettings,
    ) -> ChainStats {
        let n = input[0].len();
        let (base_drive, bias) = self.style.colour();
        let colour_target = if s.common.colour { 1.0 } else { 0.0 };
        let mut st = ChainStats::default();
        let [o0, o1] = out;
        for i in 0..n {
            if self.countdown == 0 {
                self.control(s);
                self.countdown = CONTROL;
            }
            self.countdown -= 1;
            let g = self.mixer.step(&s.common);
            let amount = self.colour.next(colour_target);
            let drive = base_drive * amount;
            let mut before = 0.0f32;
            let mut after = 0.0f32;
            for c in 0..channels {
                let x = input[c][i];
                let bands = &mut self.bands;
                let wet = self.os[c].process(x * g.gin, |v| {
                    let mut y = f64::from(v);
                    for b in bands.iter_mut() {
                        y = b.tick(c, y);
                    }
                    saturate(y as f32, drive, bias)
                });
                // The blocker removes the offset lopsided saturation makes; it fades with
                // the colour so a clean signal keeps its lowest octave untouched.
                let blocked = self.dc[c].process(wet);
                let wet = wet + (blocked - wet) * amount;
                let (pre, y) = self.mixer.blend(c, x, wet, &g, &mut st);
                before = before.max(x.abs());
                after = after.max(pre.abs());
                if c == 0 {
                    o0[i] = y;
                } else {
                    o1[i] = y;
                }
            }
            self.mixer.track(before, after);
        }
        st
    }
}
