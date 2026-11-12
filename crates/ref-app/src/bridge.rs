//! State shared with the audio thread: atomics and a lock-free reference swap only.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use arc_swap::ArcSwapOption;

/// Reference audio ready to play at the host rate.
#[derive(Debug)]
pub struct RefAudio {
    pub id: String,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// The audio-thread side of the shared state.
pub struct Bridge {
    listen_reference: AtomicBool,
    /// Reference playback gain in dB (f32 bits), already loudness matched.
    ref_gain_db: AtomicU32,
    /// Playback position the UI asks for (+1; 0 = no request).
    cue: AtomicU64,
    /// Where playback is now, for the UI.
    position: AtomicU64,
    /// Host sample rate (f64 bits); 0 before the first activation.
    host_rate: AtomicU64,
    /// Bumped on every activation so the worker knows to pick up a new mix ring.
    generation: AtomicU64,
    reference: ArcSwapOption<RefAudio>,
    /// The mix ring's consumer, parked here by the processor for the worker to take.
    pub(crate) mix_rx: Mutex<Option<rtrb::Consumer<f32>>>,
}

impl Default for Bridge {
    fn default() -> Self {
        Self {
            listen_reference: AtomicBool::new(false),
            ref_gain_db: AtomicU32::new(0f32.to_bits()),
            cue: AtomicU64::new(0),
            position: AtomicU64::new(0),
            host_rate: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            reference: ArcSwapOption::empty(),
            mix_rx: Mutex::new(None),
        }
    }
}

impl Bridge {
    /// Whether the listener hears the reference (B) instead of the mix (A).
    pub fn listening_to_reference(&self) -> bool {
        self.listen_reference.load(Ordering::Relaxed)
    }

    /// Flips between mix (false) and reference (true).
    pub fn set_listen_reference(&self, on: bool) {
        self.listen_reference.store(on, Ordering::Relaxed);
    }

    /// The reference gain the audio thread applies.
    pub fn reference_gain_db(&self) -> f32 {
        f32::from_bits(self.ref_gain_db.load(Ordering::Relaxed))
    }

    pub(crate) fn set_reference_gain_db(&self, db: f32) {
        self.ref_gain_db.store(db.to_bits(), Ordering::Relaxed);
    }

    /// The host rate, once the plugin has been activated.
    pub fn host_rate(&self) -> Option<f64> {
        let r = f64::from_bits(self.host_rate.load(Ordering::Relaxed));
        (r > 0.0).then_some(r)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn activated(&self, rate: f64, rx: rtrb::Consumer<f32>) {
        if let Ok(mut slot) = self.mix_rx.lock() {
            *slot = Some(rx);
        }
        self.host_rate.store(rate.to_bits(), Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// The reference playing now.
    pub fn reference(&self) -> arc_swap::Guard<Option<std::sync::Arc<RefAudio>>> {
        self.reference.load()
    }

    pub(crate) fn set_reference(&self, r: Option<std::sync::Arc<RefAudio>>) {
        self.reference.store(r);
        self.cue.store(1, Ordering::Relaxed);
    }

    /// Asks playback to jump to `sample`.
    pub fn cue(&self, sample: u64) {
        self.cue.store(sample.saturating_add(1), Ordering::Relaxed);
    }

    pub(crate) fn take_cue(&self) -> Option<u64> {
        let c = self.cue.swap(0, Ordering::Relaxed);
        (c > 0).then(|| c - 1)
    }

    /// Current reference playback position in samples.
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    pub(crate) fn set_position(&self, p: u64) {
        self.position.store(p, Ordering::Relaxed);
    }
}
