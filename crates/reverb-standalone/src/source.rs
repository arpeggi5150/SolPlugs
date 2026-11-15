//! What the standalone app plays through the reverb: nothing, the live input, or a
//! looping WAV file.

use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// The selected source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Silence.
    Off,
    /// The default input device.
    Live,
    /// The loaded file, looping.
    File,
}

/// A decoded stereo file at its own sample rate (played as is: the standalone is for
/// hearing the reverb, not for resampling).
pub struct File {
    left: Vec<f32>,
    right: Vec<f32>,
    rate: u32,
}

impl File {
    /// Length, seconds.
    pub fn seconds(&self) -> f32 {
        self.left.len() as f32 / self.rate.max(1) as f32
    }
}

/// Reads a WAV (16/24/32-bit int or 32-bit float), mono or stereo.
pub fn read_wav(path: &Path) -> Result<File, hound::Error> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let ch = usize::from(spec.channels).max(1);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let left: Vec<f32> = samples.chunks(ch).map(|f| f[0]).collect();
    let right: Vec<f32> = samples
        .chunks(ch)
        .map(|f| f.get(1).copied().unwrap_or(f[0]))
        .collect();
    Ok(File {
        left,
        right,
        rate: spec.sample_rate,
    })
}

/// Shared between the UI and the audio thread.
pub struct Sources {
    selected: AtomicU8,
    file: Mutex<Option<Arc<File>>>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            selected: AtomicU8::new(0),
            file: Mutex::new(None),
        }
    }
}

impl Sources {
    /// Selects a source.
    pub fn select(&self, s: Source) {
        self.selected.store(s as u8, Ordering::Relaxed);
    }

    /// The selected source.
    pub fn selected(&self) -> Source {
        match self.selected.load(Ordering::Relaxed) {
            1 => Source::Live,
            2 => Source::File,
            _ => Source::Off,
        }
    }

    /// Replaces the file.
    pub fn set_file(&self, f: File) {
        if let Ok(mut slot) = self.file.lock() {
            *slot = Some(Arc::new(f));
        }
    }

    /// The audio thread's reader. `live` carries interleaved stereo from the input.
    pub fn feed(self: &Arc<Self>, live: rtrb::Consumer<f32>) -> Feed {
        Feed {
            sources: self.clone(),
            live,
            file: None,
            pos: 0,
        }
    }
}

/// Fills blocks from the selected source (audio thread).
pub struct Feed {
    sources: Arc<Sources>,
    live: rtrb::Consumer<f32>,
    file: Option<Arc<File>>,
    pos: usize,
}

impl Feed {
    /// Fills one block.
    pub fn fill(&mut self, l: &mut [f32], r: &mut [f32]) {
        // Never blocks: if the UI is swapping the file right now, keep the old one.
        if let Ok(slot) = self.sources.file.try_lock()
            && let Some(f) = slot.as_ref()
            && !self.file.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, f))
        {
            self.file = Some(f.clone());
            self.pos = 0;
        }
        let source = self.sources.selected();
        // Keep the live ring drained even when it isn't heard, so it never lags.
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let live = (self.live.pop().ok(), self.live.pop().ok());
            (*a, *b) = match (source, live) {
                (Source::Live, (Some(x), Some(y))) => (x, y),
                _ => (0.0, 0.0),
            };
        }
        if source == Source::File
            && let Some(f) = &self.file
            && !f.left.is_empty()
        {
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                self.pos %= f.left.len();
                *a = f.left[self.pos];
                *b = f.right[self.pos];
                self.pos += 1;
            }
        }
    }
}
