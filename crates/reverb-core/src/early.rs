//! Early reflections: the first bounces off the walls, as a multi-tap delay. Left and
//! right get their own tap patterns (the walls are at different distances from each ear),
//! times grow with Size, Shape spreads them out, and each bounce is a little duller and
//! quieter than the last.

use crate::tank::one_pole;

/// Taps per side.
const TAPS: usize = 12;
/// Longest pattern at scale 1 and full spread, ms.
const SPAN_MS: f32 = 90.0;

/// The tap pattern: (time 0..1 of the span, gain, sign) for left; right is offset.
fn pattern(k: usize, right: bool) -> (f32, f32) {
    let t = (k as f32 + 0.6) / TAPS as f32;
    let jitter = if right {
        0.07 * (k as f32 * 1.93).cos()
    } else {
        0.06 * (k as f32 * 2.71).sin()
    };
    let time = (t.powf(1.35) + jitter * t).clamp(0.01, 1.0);
    let sign = if (k + usize::from(right)).is_multiple_of(3) {
        -1.0
    } else {
        1.0
    };
    (time, sign * 0.88f32.powi(k as i32))
}

/// The reflections.
pub struct Early {
    sr: f32,
    buf: [Vec<f32>; 2],
    pos: usize,
    taps: [[(usize, f32); TAPS]; 2],
    lp: [f32; 2],
    a_lp: f32,
}

impl Early {
    /// Reflections at `sr`.
    pub fn new(sr: f32) -> Self {
        let max = (SPAN_MS * 1.2 * crate::tank::MAX_SCALE * 0.001 * sr) as usize + 4;
        Self {
            sr,
            buf: [vec![0.0; max], vec![0.0; max]],
            pos: 0,
            taps: [[(1, 0.0); TAPS]; 2],
            lp: [0.0; 2],
            a_lp: 0.5,
        }
    }

    /// Clears the history.
    pub fn reset(&mut self) {
        for b in &mut self.buf {
            b.fill(0.0);
        }
        self.lp = [0.0; 2];
    }

    /// Sets the pattern: `scale` the room size, `spread` 0..1 (Shape), `high_damp` 0..1.
    pub fn set(&mut self, scale: f32, spread: f32, high_damp: f32) {
        let span = SPAN_MS
            * scale.clamp(0.05, crate::tank::MAX_SCALE)
            * (0.35 + 0.85 * spread.clamp(0.0, 1.0));
        let max = self.buf[0].len() - 2;
        let mut sum = 0.0f32;
        for side in 0..2 {
            for k in 0..TAPS {
                let (t, g) = pattern(k, side == 1);
                let n = ((t * span * 0.001 * self.sr) as usize).clamp(1, max);
                self.taps[side][k] = (n, g);
                if side == 0 {
                    sum += g * g;
                }
            }
        }
        // Keep the reflections' energy near the input's.
        let norm = 1.0 / sum.sqrt();
        for side in &mut self.taps {
            for t in side.iter_mut() {
                t.1 *= norm;
            }
        }
        self.a_lp = one_pole(crate::tank::damping_hz(high_damp * 0.7), self.sr);
    }

    /// One stereo sample in, the reflections out. `cross`: each side also hears the other
    /// wall (off in dual mono, so the sides stay apart).
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32, cross: bool) -> (f32, f32) {
        let n = self.buf[0].len();
        self.pos = (self.pos + 1) % n;
        self.buf[0][self.pos] = l;
        self.buf[1][self.pos] = r;
        let mut out = [0.0f32; 2];
        for side in 0..2 {
            // Each side hears mostly its own wall, a little of the other.
            let (own, other) = (side, 1 - side);
            let mut acc = 0.0;
            for (k, &(d, g)) in self.taps[side].iter().enumerate() {
                let i = (self.pos + n - d) % n;
                let src = if cross && k % 4 == 3 { other } else { own };
                acc += self.buf[src][i] * g;
            }
            self.lp[side] += (acc - self.lp[side]) * self.a_lp;
            out[side] = self.lp[side];
        }
        (out[0], out[1])
    }
}
