//! The preview player. The UI hands it a clip; the audio thread (the plugin's process call,
//! or the standalone window's output stream) mixes it into the output.
//!
//! Realtime-safe: the clip is read through a lock-free swap and positions are atomics. Clips
//! the UI replaces are kept alive on the UI side for a moment, so the audio thread never frees
//! one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use arc_swap::ArcSwapOption;
use solplugs_samples_core::audio::Audio;

/// What the player is playing.
pub struct Clip {
    /// The sample id it came from.
    pub id: String,
    /// The audio as heard: the sample through its processing.
    pub audio: Arc<Audio>,
}

/// Shared between the UI and the audio thread.
pub struct Player {
    clip: ArcSwapOption<Clip>,
    /// Play position in the clip's frames, as f64 bits.
    pos: AtomicU64,
    /// Play range end (frames); playback stops or loops here.
    end: AtomicU64,
    /// Loop start (frames).
    start: AtomicU64,
    playing: AtomicBool,
    looping: AtomicBool,
    /// Linear gain as f32 bits.
    gain: AtomicU32,
    /// Output sample rate as f64 bits (set by whoever renders).
    out_rate: AtomicU64,
    retired: std::sync::Mutex<Vec<(std::time::Instant, Arc<Clip>)>>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            clip: ArcSwapOption::empty(),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(u64::MAX),
            start: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            gain: AtomicU32::new(1f32.to_bits()),
            out_rate: AtomicU64::new(48_000f64.to_bits()),
            retired: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Player {
    /// Starts playing `clip` from `from` seconds to `to` seconds (or the end).
    pub fn play(&self, clip: Arc<Clip>, from: f64, to: Option<f64>) {
        let sr = clip.audio.sample_rate;
        let n = clip.audio.frames() as u64;
        let start = ((from.max(0.0) * sr) as u64).min(n);
        let end = to.map_or(n, |t| ((t * sr) as u64).clamp(start, n));
        self.playing.store(false, Ordering::Release);
        self.pos.store((start as f64).to_bits(), Ordering::Release);
        self.start.store(start, Ordering::Release);
        self.end.store(end, Ordering::Release);
        self.swap(Some(clip));
        self.playing.store(true, Ordering::Release);
    }

    /// Replaces the clip's audio (after a processing change) without moving the playhead.
    pub fn replace(&self, clip: Arc<Clip>) {
        let n = clip.audio.frames() as u64;
        if self.end.load(Ordering::Acquire) > n {
            self.end.store(n, Ordering::Release);
        }
        self.swap(Some(clip));
    }

    fn swap(&self, clip: Option<Arc<Clip>>) {
        let old = self.clip.swap(clip);
        if let Ok(mut r) = self.retired.lock() {
            let now = std::time::Instant::now();
            r.retain(|(t, _)| now.duration_since(*t).as_secs_f64() < 2.0);
            if let Some(o) = old {
                r.push((now, o));
            }
        }
    }

    /// Stops.
    pub fn stop(&self) {
        self.playing.store(false, Ordering::Release);
    }

    /// True while playing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Acquire)
    }

    /// Loop the play range.
    pub fn set_looping(&self, on: bool) {
        self.looping.store(on, Ordering::Release);
    }

    /// True when looping.
    #[must_use]
    pub fn looping(&self) -> bool {
        self.looping.load(Ordering::Acquire)
    }

    /// Preview level in dB.
    pub fn set_gain_db(&self, db: f32) {
        self.gain
            .store(10f32.powf(db / 20.0).to_bits(), Ordering::Release);
    }

    /// The id of the clip loaded, if any.
    #[must_use]
    pub fn clip_id(&self) -> Option<String> {
        self.clip.load().as_ref().map(|c| c.id.clone())
    }

    /// The clip loaded, if any (UI side).
    #[must_use]
    pub fn clip(&self) -> Option<Arc<Clip>> {
        self.clip.load_full()
    }

    /// Position in seconds within the clip.
    #[must_use]
    pub fn position_sec(&self) -> f64 {
        let sr = self
            .clip
            .load()
            .as_ref()
            .map_or(48_000.0, |c| c.audio.sample_rate);
        f64::from_bits(self.pos.load(Ordering::Acquire)) / sr.max(1.0)
    }

    /// The output sample rate last used.
    #[must_use]
    pub fn output_rate(&self) -> f64 {
        f64::from_bits(self.out_rate.load(Ordering::Acquire))
    }

    /// Mixes the preview into `left`/`right` at `out_rate`. Audio thread; never blocks or
    /// allocates.
    pub fn render(&self, left: &mut [f32], right: &mut [f32], out_rate: f64) {
        if out_rate > 0.0 {
            self.out_rate.store(out_rate.to_bits(), Ordering::Relaxed);
        }
        if !self.playing.load(Ordering::Acquire) {
            return;
        }
        let guard = self.clip.load();
        let Some(clip) = guard.as_ref() else {
            return;
        };
        let a = &clip.audio;
        let Some(ch_l) = a.channels.first() else {
            return;
        };
        let ch_r = a.channels.get(1).unwrap_or(ch_l);
        let step = a.sample_rate / out_rate.max(1.0);
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        let end = (self.end.load(Ordering::Acquire) as usize).min(ch_l.len());
        let start = self.start.load(Ordering::Acquire) as usize;
        let looping = self.looping.load(Ordering::Acquire);
        let mut pos = f64::from_bits(self.pos.load(Ordering::Acquire));
        let n = left.len().min(right.len());
        for i in 0..n {
            if pos >= end as f64 {
                if looping && end > start {
                    pos = start as f64 + (pos - end as f64);
                } else {
                    self.playing.store(false, Ordering::Release);
                    break;
                }
            }
            let k = pos as usize;
            let frac = (pos - k as f64) as f32;
            let k1 = (k + 1).min(end.saturating_sub(1)).max(k);
            let l = ch_l[k] + (ch_l[k1] - ch_l[k]) * frac;
            let r = ch_r[k] + (ch_r[k1] - ch_r[k]) * frac;
            left[i] += l * gain;
            right[i] += r * gain;
            pos += step;
        }
        self.pos.store(pos.to_bits(), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_resampled_and_stops_at_the_end() {
        let p = Player::default();
        let audio = Arc::new(Audio::new(24_000.0, vec![vec![0.5; 100]]));
        p.play(
            Arc::new(Clip {
                id: "a".into(),
                audio,
            }),
            0.0,
            None,
        );
        let (mut l, mut r) = (vec![0.0; 300], vec![0.0; 300]);
        p.render(&mut l, &mut r, 48_000.0);
        // 100 frames at 24 kHz is 200 frames at 48 kHz.
        assert_eq!(l[199], 0.5);
        assert_eq!(r[10], 0.5);
        assert_eq!(l[200], 0.0);
        assert!(!p.is_playing());
    }

    #[test]
    fn loops_the_range() {
        let p = Player::default();
        let audio = Arc::new(Audio::new(
            1000.0,
            vec![(0..1000).map(|i| i as f32).collect()],
        ));
        p.set_looping(true);
        p.play(
            Arc::new(Clip {
                id: "a".into(),
                audio,
            }),
            0.1,
            Some(0.2),
        );
        let (mut l, mut r) = (vec![0.0; 250], vec![0.0; 250]);
        p.render(&mut l, &mut r, 1000.0);
        assert_eq!(l[0], 100.0);
        assert_eq!(l[100], 100.0);
        assert!(p.is_playing());
    }
}
