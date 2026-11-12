//! The audio-thread processor: passes the mix through untouched, copies it to the analyzer,
//! and crossfades to the loudness-matched reference when B is selected.
//!
//! Zero latency: nothing is delayed. Realtime-safe: buffers are allocated in [`Processor::new`],
//! and the reference is read through a lock-free guard.

use std::sync::Arc;

use solplugs_ref_core::abswitch::{AbSwitch, Source};

use crate::bridge::Bridge;

/// Crossfade time for A/B flips.
pub const FLIP_MS: f64 = 25.0;
/// Seconds of mix the ring buffer holds for the worker.
const RING_SECONDS: f64 = 2.0;

/// One processor per activation.
pub struct Processor {
    bridge: Arc<Bridge>,
    tx: rtrb::Producer<f32>,
    ab: AbSwitch,
    pos: usize,
    ref_l: Vec<f32>,
    ref_r: Vec<f32>,
}

impl Processor {
    /// Creates the processor for an activation at `sample_rate` with blocks up to `max_block`.
    #[must_use]
    pub fn new(bridge: Arc<Bridge>, sample_rate: f64, max_block: usize) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let (tx, rx) = rtrb::RingBuffer::new((sr * RING_SECONDS) as usize * 2);
        bridge.activated(sr, rx);
        let block = max_block.clamp(32, 1 << 16);
        let mut ab = AbSwitch::new(sr, FLIP_MS);
        if bridge.listening_to_reference() {
            ab.set_source(Source::Reference);
        }
        Self { bridge, tx, ab, pos: 0, ref_l: vec![0.0; block], ref_r: vec![0.0; block] }
    }

    /// Processes one block in place. `left`/`right` hold the mix on entry.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        // 1. The mix goes to the analyzer exactly as it arrives (dropped if the worker lags).
        let want = (n * 2).min(self.tx.slots() / 2 * 2);
        if let Ok(mut chunk) = self.tx.write_chunk(want) {
            let mut it = left[..want / 2].iter().zip(&right[..want / 2]).flat_map(|(&l, &r)| [l, r]);
            let (a, b) = chunk.as_mut_slices();
            for slot in a.iter_mut().chain(b.iter_mut()) {
                *slot = it.next().unwrap_or(0.0);
            }
            chunk.commit_all();
        }
        // 2. A/B.
        let target = if self.bridge.listening_to_reference() { Source::Reference } else { Source::Mix };
        self.ab.set_source(target);
        self.ab.set_reference_gain_db(f64::from(self.bridge.reference_gain_db()));
        let guard = self.bridge.reference();
        let reference = guard.as_ref().filter(|r| !r.left.is_empty() && r.left.len() == r.right.len());
        if let Some(cue) = self.bridge.take_cue() {
            self.pos = cue as usize;
        }
        let mut done = 0;
        while done < n {
            let len = (n - done).min(self.ref_l.len());
            let (rl, rr) = (&mut self.ref_l[..len], &mut self.ref_r[..len]);
            match reference {
                Some(r) if self.ab.reference_audible() => {
                    let total = r.left.len();
                    for i in 0..len {
                        if self.pos >= total {
                            self.pos = 0;
                        }
                        rl[i] = r.left[self.pos];
                        rr[i] = r.right[self.pos];
                        self.pos += 1;
                    }
                }
                Some(r) => {
                    // Keep time while A is heard, so flipping lands where the reference would be.
                    self.pos = (self.pos + len) % r.left.len();
                    rl.fill(0.0);
                    rr.fill(0.0);
                }
                None => {
                    rl.fill(0.0);
                    rr.fill(0.0);
                }
            }
            self.ab.process(&mut left[done..done + len], &mut right[done..done + len], rl, rr);
            done += len;
        }
        self.bridge.set_position(self.pos as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::RefAudio;

    #[test]
    fn passes_the_mix_through_and_feeds_the_analyzer() {
        let bridge = Arc::new(Bridge::default());
        let mut p = Processor::new(bridge.clone(), 48_000.0, 512);
        let mut l = vec![0.25f32; 256];
        let mut r = vec![-0.25f32; 256];
        p.process(&mut l, &mut r);
        assert!(l.iter().all(|&x| x == 0.25) && r.iter().all(|&x| x == -0.25));
        let mut rx = bridge.mix_rx.lock().ok().and_then(|mut s| s.take());
        let got = rx.as_mut().map_or(0, |rx| rx.slots());
        assert_eq!(got, 512);
    }

    #[test]
    fn b_plays_the_reference_with_its_gain_and_loops() {
        let bridge = Arc::new(Bridge::default());
        bridge.set_reference(Some(Arc::new(RefAudio { id: "x".into(), left: vec![0.5; 1000], right: vec![0.5; 1000] })));
        bridge.set_reference_gain_db(0.0);
        bridge.set_listen_reference(true);
        let mut p = Processor::new(bridge.clone(), 48_000.0, 512);
        let mut last = 0.0;
        for _ in 0..20 {
            let mut l = vec![0.1f32; 512];
            let mut r = vec![0.1f32; 512];
            p.process(&mut l, &mut r);
            last = l[511];
        }
        assert!((last - 0.5).abs() < 1e-3, "{last}");
        assert!(bridge.position() < 1000);
    }
}
