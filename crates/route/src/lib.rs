//! SolRoute feed-in: hear and analyse whatever is playing on the machine (a streaming app,
//! a browser tab, a video) as a reference, without any extra plugin.
//!
//! On Windows this is WASAPI loopback capture of an output device. The captured audio goes
//! through a lock-free ring buffer to whoever drains it (the analyzer). It is kept in memory
//! only; nothing here writes audio to disk.
//!
//! When Solara's own SolRoute router is running, its virtual output shows up here as just
//! another output device, so the same capture path covers it.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

/// Seconds of audio the ring buffer holds between drains.
pub const RING_SECONDS: usize = 4;

/// Why capture could not start.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("system audio capture isn't available on this platform yet")]
    Unsupported,
    #[error("no audio output device found")]
    NoDevice,
    #[error("the audio device refused: {0}")]
    Device(String),
}

/// An output device that can be captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Stable id to pass to [`Feed::start`].
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// A running capture. Dropping it stops the capture.
pub struct Feed {
    rx: rtrb::Consumer<f32>,
    sample_rate: f64,
    name: String,
    #[cfg(windows)]
    _stream: cpal::Stream,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
}

impl Feed {
    /// The capture's sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// The captured device's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Hands every captured frame since the last call to `sink` as left/right slices.
    pub fn drain(&mut self, mut sink: impl FnMut(&[f32], &[f32])) {
        let available = self.rx.slots() / 2 * 2;
        if available == 0 {
            return;
        }
        self.scratch_l.clear();
        self.scratch_r.clear();
        if let Ok(chunk) = self.rx.read_chunk(available) {
            let (a, b) = chunk.as_slices();
            let mut it = a.iter().chain(b.iter());
            while let (Some(&l), Some(&r)) = (it.next(), it.next()) {
                self.scratch_l.push(l);
                self.scratch_r.push(r);
            }
            chunk.commit_all();
        }
        sink(&self.scratch_l, &self.scratch_r);
    }
}

/// Lists capturable output devices.
pub fn sources() -> Result<Vec<Source>, RouteError> {
    imp::sources()
}

impl Feed {
    /// Starts capturing the device with `id`, or the default output when `None`.
    pub fn start(id: Option<&str>) -> Result<Self, RouteError> {
        imp::start(id)
    }
}

/// Pushes interleaved frames of any channel count into the ring as stereo, dropping what
/// doesn't fit (the reader fell behind; losing audio beats blocking the device thread).
#[cfg_attr(not(windows), allow(dead_code))]
fn push_stereo(tx: &mut rtrb::Producer<f32>, data: &[f32], channels: usize) {
    let channels = channels.max(1);
    for frame in data.chunks_exact(channels) {
        if tx.slots() < 2 {
            return;
        }
        let l = frame[0];
        let r = if channels > 1 { frame[1] } else { frame[0] };
        let _ = tx.push(l);
        let _ = tx.push(r);
    }
}

#[cfg(windows)]
mod imp {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::{Feed, RING_SECONDS, RouteError, Source, push_stereo};

    fn dev_err(e: impl std::fmt::Display) -> RouteError {
        RouteError::Device(e.to_string())
    }

    fn id_of(d: &cpal::Device) -> String {
        d.id()
            .map(|i| i.to_string())
            .unwrap_or_else(|_| d.to_string())
    }

    pub fn sources() -> Result<Vec<Source>, RouteError> {
        let host = cpal::default_host();
        let default = host.default_output_device().map(|d| id_of(&d));
        Ok(host
            .output_devices()
            .map_err(dev_err)?
            .map(|d| {
                let id = id_of(&d);
                Source {
                    is_default: Some(&id) == default.as_ref(),
                    name: d.to_string(),
                    id,
                }
            })
            .collect())
    }

    pub fn start(id: Option<&str>) -> Result<Feed, RouteError> {
        let host = cpal::default_host();
        let device = match id {
            Some(want) => host
                .output_devices()
                .map_err(dev_err)?
                .find(|d| id_of(d) == want),
            None => host.default_output_device(),
        }
        .ok_or(RouteError::NoDevice)?;
        let supported = device.default_output_config().map_err(dev_err)?;
        let config = supported.config();
        let channels = usize::from(config.channels);
        let sample_rate = f64::from(config.sample_rate);
        let (mut tx, rx) = rtrb::RingBuffer::new(sample_rate as usize * 2 * RING_SECONDS);
        let err = |e: cpal::Error| log::warn!("SolRoute capture: {e}");
        // Building an input stream on an output device is WASAPI loopback.
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    push_stereo(&mut tx, data, channels)
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut buf = Vec::with_capacity(8192);
                device.build_input_stream(
                    config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        buf.clear();
                        buf.extend(data.iter().map(|&s| f32::from(s) / 32_768.0));
                        push_stereo(&mut tx, &buf, channels);
                    },
                    err,
                    None,
                )
            }
            other => {
                return Err(RouteError::Device(format!(
                    "unsupported sample format {other:?}"
                )));
            }
        }
        .map_err(dev_err)?;
        stream.play().map_err(dev_err)?;
        Ok(Feed {
            rx,
            sample_rate,
            name: device.to_string(),
            _stream: stream,
            scratch_l: Vec::with_capacity(sample_rate as usize * RING_SECONDS),
            scratch_r: Vec::with_capacity(sample_rate as usize * RING_SECONDS),
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Feed, RouteError, Source};

    pub fn sources() -> Result<Vec<Source>, RouteError> {
        Err(RouteError::Unsupported)
    }

    pub fn start(_id: Option<&str>) -> Result<Feed, RouteError> {
        Err(RouteError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_stereo_folds_channels_and_drops_when_full() {
        let (mut tx, mut rx) = rtrb::RingBuffer::<f32>::new(6);
        // 4 frames of 3-channel audio; only 3 stereo frames fit.
        push_stereo(
            &mut tx,
            &[1.0, 2.0, 9.0, 3.0, 4.0, 9.0, 5.0, 6.0, 9.0, 7.0, 8.0, 9.0],
            3,
        );
        let got: Vec<f32> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert_eq!(got, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let (mut tx, mut rx) = rtrb::RingBuffer::<f32>::new(8);
        push_stereo(&mut tx, &[0.5, 0.25], 1);
        let got: Vec<f32> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert_eq!(got, vec![0.5, 0.5, 0.25, 0.25]);
    }

    /// Live check: play audio on the default output, then
    /// `cargo test -p solplugs-route -- --ignored --nocapture route_live`.
    #[test]
    #[ignore = "needs a sound device with audio playing"]
    #[allow(clippy::expect_used)]
    fn route_live_captures_system_audio() {
        let mut feed = Feed::start(None).expect("start capture");
        println!("capturing {} at {} Hz", feed.name(), feed.sample_rate());
        let mut frames = 0usize;
        let mut sum = 0.0f64;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < until {
            feed.drain(|l, r| {
                frames += l.len();
                sum += l
                    .iter()
                    .chain(r)
                    .map(|x| f64::from(*x) * f64::from(*x))
                    .sum::<f64>();
            });
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let rms = (sum / (2 * frames.max(1)) as f64).sqrt();
        println!("captured {frames} frames, rms {rms:.4}");
        assert!(frames as f64 > feed.sample_rate() * 2.0, "too few frames");
        assert!(rms > 1e-4, "captured silence; is anything playing?");
    }
}
