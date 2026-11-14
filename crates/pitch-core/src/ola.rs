//! Pitch-synchronous overlap-add, the synthesis half of TD-PSOLA (E. Moulines and
//! F. Charpentier, "Pitch-synchronous waveform processing techniques for text-to-speech
//! synthesis using diphones", Speech Communication 9, 1990), implemented from the paper.
//!
//! A grain is a Hann-windowed slice of the source centred on an analysis mark, two periods
//! long. Re-spacing grains at the output changes the pitch; because each grain keeps its
//! own waveform, the spectral envelope (the formants) stays where it was. Reading the
//! grain at a rate `q` other than 1 moves the formants by `q` (used for formant shift and
//! for the "formant off" chipmunk mode).
//!
//! The output is divided by the summed window weight wherever that sum is above
//! [`NORM_FLOOR`], so grains placed exactly where they came from reproduce the source
//! bit-for-bit (up to float rounding), whatever the local period.

/// Window-sum floor for normalisation. Below it (between grains when shifting down) the
/// output fades instead of amplifying grain tails.
pub const NORM_FLOOR: f32 = 0.5;

/// Hann weight at offset `i` of a grain with half-length `h`: 1 at the centre, 0 at ±h.
#[inline]
pub fn hann(i: f32, h: f32) -> f32 {
    0.5 + 0.5 * (std::f32::consts::PI * i / h).cos()
}

/// Cubic (Catmull-Rom) read of `ring` at fractional position `pos`.
#[inline]
pub fn read_cubic(ring: &[f32], mask: usize, pos: f64) -> f32 {
    let i = pos.floor();
    let f = (pos - i) as f32;
    let i = i as i64;
    let at = |k: i64| ring[(k as usize) & mask];
    let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * f + c2) * f + c1) * f + y1
}

/// One grain to place: output centre `out`, source centre `src`, output half-lengths
/// `left` and `right`, source read rate `rate`.
///
/// The halves are asymmetric: each reaches to the neighbouring grain's centre, so two
/// neighbours' facing halves are the same length and their raised cosines sum to exactly 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    /// Output position of the grain centre (absolute samples).
    pub out: i64,
    /// Source position of the grain centre (absolute samples, fractional allowed).
    pub src: f64,
    /// Left half-length at the output, samples.
    pub left: usize,
    /// Right half-length at the output, samples.
    pub right: usize,
    /// Source samples read per output sample (1 = formants preserved).
    pub rate: f32,
}

/// Adds `g` to the output accumulator `acc` (ring with `out_mask`) reading the source
/// ring `src` (with `src_mask`). When `wsum` is given the window weights are added to it.
pub fn add_grain(
    g: &Grain,
    src: &[f32],
    src_mask: usize,
    acc: &mut [f32],
    out_mask: usize,
    wsum: Option<&mut [f32]>,
) {
    let hl = g.left.max(1) as i64;
    let hr = g.right.max(1) as i64;
    let integral = g.rate == 1.0 && g.src.fract() == 0.0;
    let s0 = g.src as i64;
    let mut wsum = wsum;
    for i in (1 - hl)..hr {
        let w = if i < 0 {
            hann(i as f32, hl as f32)
        } else {
            hann(i as f32, hr as f32)
        };
        let x = if integral {
            src[((s0 + i) as usize) & src_mask]
        } else {
            read_cubic(src, src_mask, g.src + f64::from(g.rate) * i as f64)
        };
        let o = ((g.out + i) as usize) & out_mask;
        acc[o] += w * x;
        if let Some(ws) = wsum.as_deref_mut() {
            ws[o] += w;
        }
    }
}

/// The normalised output sample from an accumulator and its window sum.
#[inline]
pub fn normalise(acc: f32, wsum: f32) -> f32 {
    acc / wsum.max(NORM_FLOOR)
}
