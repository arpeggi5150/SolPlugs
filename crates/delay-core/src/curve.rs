//! The repeat picture the delay window draws: where each repeat lands and how loud it is,
//! left and right, from the settings alone.

use crate::{Extra, Version, VersionSettings};

/// One repeat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repeat {
    /// When it lands, ms after the dry hit.
    pub t_ms: f32,
    /// Level on the left, dB.
    pub left_db: f32,
    /// Level on the right, dB.
    pub right_db: f32,
    /// How much of the top end is left, 0..1 (1 = full).
    pub bright: f32,
}

/// Extra loss per pass through a version's loop at mid frequencies, dB.
fn pass_loss_db(v: Version, s: &VersionSettings) -> f32 {
    match v {
        Version::Clean => 0.0,
        Version::Tape => 0.4 + 0.6 * s.extra(v, Extra::Saturation) / 100.0,
        Version::Analog => 0.8 + 0.8 * s.extra(v, Extra::Drive) / 100.0,
    }
}

/// How much brightness each pass keeps (the loop's low-pass, the tape head, the chip).
fn pass_bright(v: Version, s: &VersionSettings, delay_ms: f32) -> f32 {
    let lp = (s.filter_lp / 20_000.0).clamp(0.02, 1.0).powf(0.35);
    let own = match v {
        Version::Clean => 1.0,
        Version::Tape => 0.8,
        Version::Analog => (0.62 * (250.0 / delay_ms.max(1.0)).sqrt()).clamp(0.4, 0.8),
    };
    lp * own
}

/// The repeats that land within `window_ms`, quieter than -60 dB left out. The ping-pong
/// delay's repeats alternate sides; the stereo delay's land on both (the right one later by
/// the offset).
pub fn repeats(v: Version, s: &VersionSettings, bpm: f32, window_ms: f32) -> Vec<Repeat> {
    let d = s.delay_ms(bpm);
    let fb = s.feedback.clamp(0.0, 1.2);
    let loss = pass_loss_db(v, s);
    let keep = pass_bright(v, s, d);
    let db = |g: f32| if g <= 1e-6 { -120.0 } else { 20.0 * g.log10() };
    let mut out = Vec::new();
    let max_n: i32 = 64;
    for k in 1..=max_n {
        let kf = k as f32;
        let t = d * kf;
        if t > window_ms {
            break;
        }
        // Self-oscillation holds at full level instead of growing.
        let g = db(fb.min(1.0)) * (kf - 1.0) - loss * (kf - 1.0);
        let st = g + db(s.stereo_mix.max(1e-6));
        let pp = g + db(s.pingpong_mix.max(1e-6));
        let pp_left = k % 2 == 1;
        let combine = |a: f32, b: f32| {
            let p = 10f32.powf(a / 10.0) + 10f32.powf(b / 10.0);
            10.0 * p.max(1e-12).log10()
        };
        let left = if pp_left { combine(st, pp) } else { st };
        let right = if pp_left { st } else { combine(st, pp) };
        if left.max(right) < -60.0 {
            break;
        }
        out.push(Repeat {
            t_ms: t,
            left_db: left.max(-120.0),
            right_db: right.max(-120.0),
            bright: keep.powi(k),
        });
    }
    out
}

/// Roughly how many repeats ring before they fall 60 dB ("ringing" on the feedback meter);
/// `None` when it self-oscillates.
pub fn ringing(v: Version, s: &VersionSettings) -> Option<f32> {
    let fb = s.feedback.clamp(0.0, 1.2);
    let per = -20.0 * fb.max(1e-6).log10() + pass_loss_db(v, s);
    if per <= 0.05 {
        None
    } else {
        Some((60.0 / per).min(999.0))
    }
}
