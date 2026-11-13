//! SolPlugs MIDI Keys: play synths from hardware MIDI keyboards.
//!
//! One to four tracks. Each track listens to one MIDI input (a keyboard, or any input) on one
//! channel (or all of them), and plays one synth: a VST3 instrument (hosted by Solara's
//! `soundcraft-vst3-host`) or one of Solara's built-in instruments. The tracks are mixed to the
//! audio output with a small buffer, so a key sounds with no noticeable lag.
//!
//! Shared piece: Command Center's MIDI module drives the `solplug-midi-keys` program built on this
//! crate, and Solara can use the same [`Mixer`] and [`route`] for its own hardware MIDI input
//! (one keyboard implementation, not two).
//!
//! Layout: the routing and the mixer are pure and tested on every platform; the devices (MIDI in
//! through WinMM, audio out through WASAPI) live in [`devices`], on Windows.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

#[cfg(windows)]
pub mod devices;

use serde::{Deserialize, Serialize};
use soundcraft_dsp::Plugin;

/// At most this many tracks (and so at most this many inputs in use).
pub const MAX_TRACKS: usize = 4;
/// The largest block the synths are prepared for; longer device buffers are rendered in pieces.
pub const MAX_BLOCK: usize = 512;

/// One track as the user set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrackCfg {
    /// The MIDI input's name; `None` (or empty) listens to every input.
    pub input: Option<String>,
    /// MIDI channel 1-16; `None` or 0 listens to all channels.
    pub channel: Option<u8>,
    /// The synth: `vst3:<class id>` or a built-in instrument id. `None`: the track is silent.
    pub synth: Option<String>,
    /// 0.0 - 1.0 (values outside are clamped; NaN is 0.8).
    pub volume: Option<f32>,
    pub mute: bool,
}

impl TrackCfg {
    /// The track's gain, clamped (NaN or missing: 0.8).
    pub fn gain(&self) -> f32 {
        match self.volume {
            Some(v) if v.is_finite() => v.clamp(0.0, 1.0),
            _ => 0.8,
        }
    }
    fn hears(&self, input: &str, channel: u8) -> bool {
        let input_ok = match self.input.as_deref() {
            None | Some("") => true,
            Some(want) => want == input,
        };
        let channel_ok = match self.channel {
            None | Some(0) => true,
            Some(c) => c == channel,
        };
        input_ok && channel_ok
    }
}

/// What a MIDI message does to a synth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    On {
        note: u8,
        velocity: u8,
    },
    Off {
        note: u8,
    },
    /// Control change, pitch bend, pressure, program change: passed to the synth as is.
    Other([u8; 3]),
    /// All notes off (controller 123 or 120).
    AllOff,
}

/// Reads one MIDI 1.0 channel message: its channel (1-16) and what it does. System messages
/// (clock, active sensing, SysEx) and broken messages are `None`.
pub fn parse(bytes: &[u8]) -> Option<(u8, Event)> {
    let status = *bytes.first()?;
    if !(0x80..0xF0).contains(&status) {
        return None;
    }
    let channel = (status & 0x0F) + 1;
    let d1 = bytes.get(1).copied().unwrap_or(0) & 0x7F;
    let d2 = bytes.get(2).copied().unwrap_or(0) & 0x7F;
    let ev = match status & 0xF0 {
        0x90 if bytes.len() >= 3 && d2 > 0 => Event::On {
            note: d1,
            velocity: d2,
        },
        0x90 | 0x80 if bytes.len() >= 3 => Event::Off { note: d1 },
        0xB0 if bytes.len() >= 3 && (d1 == 123 || d1 == 120) => Event::AllOff,
        0xB0 | 0xE0 | 0xA0 if bytes.len() >= 3 => Event::Other([status, d1, d2]),
        0xC0 | 0xD0 if bytes.len() >= 2 => Event::Other([status, d1, 0]),
        _ => return None,
    };
    Some((channel, ev))
}

/// The tracks a message from `input` on `channel` plays (each track listens to its own input).
pub fn route(tracks: &[TrackCfg], input: &str, channel: u8) -> Vec<usize> {
    tracks
        .iter()
        .take(MAX_TRACKS)
        .enumerate()
        .filter(|(_, t)| t.hears(input, channel))
        .map(|(i, _)| i)
        .collect()
}

/// The synths of a rig as the audio thread plays them.
pub struct Mixer {
    slots: Vec<Option<Slot>>,
    rate: f32,
}

struct Slot {
    synth: Box<dyn Plugin>,
    gain: f32,
    mute: bool,
    buf: Vec<Vec<f32>>,
}

impl Mixer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            slots: (0..MAX_TRACKS).map(|_| None).collect(),
            rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
        }
    }
    pub fn sample_rate(&self) -> f32 {
        self.rate
    }
    /// Puts a synth on a track (prepared here, so call it off the audio thread and hand the mixer
    /// over afterwards, or use [`Mixer::take`] / [`Mixer::put`] through a queue). `None` empties it.
    /// Returns the synth that was there, for dropping off the audio thread.
    pub fn set(
        &mut self,
        track: usize,
        synth: Option<Box<dyn Plugin>>,
        cfg: &TrackCfg,
    ) -> Option<Box<dyn Plugin>> {
        let slot = self.slots.get_mut(track)?;
        let old = slot.take().map(|s| s.synth);
        *slot = synth.map(|mut synth| {
            synth.prepare(self.rate, MAX_BLOCK, 2);
            Slot {
                synth,
                gain: cfg.gain(),
                mute: cfg.mute,
                buf: vec![vec![0.0; MAX_BLOCK]; 2],
            }
        });
        old
    }
    /// A synth made ready for this mixer's rate, off the audio thread, for [`Mixer::put`].
    pub fn prepare(&self, mut synth: Box<dyn Plugin>, cfg: &TrackCfg) -> Prepared {
        synth.prepare(self.rate, MAX_BLOCK, 2);
        Prepared(Slot {
            synth,
            gain: cfg.gain(),
            mute: cfg.mute,
            buf: vec![vec![0.0; MAX_BLOCK]; 2],
        })
    }
    /// Puts a prepared synth on a track (or empties it) without allocating: safe on the audio
    /// thread. The one that was there comes back, to be dropped elsewhere.
    pub fn put(&mut self, track: usize, slot: Option<Prepared>) -> Option<Prepared> {
        let s = self.slots.get_mut(track)?;
        let old = s.take().map(Prepared);
        *s = slot.map(|p| p.0);
        old
    }
    /// Volume and mute only (no new synth): safe on the audio thread.
    pub fn level(&mut self, track: usize, gain: f32, mute: bool) {
        if let Some(Some(s)) = self.slots.get_mut(track) {
            s.gain = if gain.is_finite() {
                gain.clamp(0.0, 1.0)
            } else {
                0.8
            };
            s.mute = mute;
        }
    }
    /// Takes a prepared slot out (to move it into another mixer).
    pub fn has(&self, track: usize) -> bool {
        matches!(self.slots.get(track), Some(Some(_)))
    }
    /// An event for a track, at the start of the next block.
    pub fn event(&mut self, track: usize, ev: Event) {
        let Some(Some(s)) = self.slots.get_mut(track) else {
            return;
        };
        match ev {
            Event::On { note, velocity } => s.synth.note_on(0, note, velocity),
            Event::Off { note } => s.synth.note_off(0, note),
            Event::Other(m) => s.synth.midi(0, m),
            Event::AllOff => s.synth.all_notes_off(),
        }
    }
    /// Silences every track (stop, or a panic button).
    pub fn all_off(&mut self) {
        for s in self.slots.iter_mut().flatten() {
            s.synth.all_notes_off();
        }
    }
    /// Renders interleaved output with `channels` channels (the synths play stereo; a mono device
    /// gets the left side, extra channels stay silent). Does not allocate.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let ch = channels.max(1);
        let frames = out.len() / ch;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX_BLOCK);
            for s in self.slots.iter_mut().flatten() {
                for b in s.buf.iter_mut() {
                    if let Some(x) = b.get_mut(..n) {
                        x.fill(0.0);
                    }
                }
                s.synth.process(&mut s.buf, n);
                if s.mute {
                    continue;
                }
                for f in 0..n {
                    for c in 0..ch.min(2) {
                        let v =
                            s.buf.get(c).and_then(|b| b.get(f)).copied().unwrap_or(0.0) * s.gain;
                        if let Some(o) = out.get_mut((done + f) * ch + c) {
                            *o += if v.is_finite() { v } else { 0.0 };
                        }
                    }
                }
            }
            done += n;
        }
        for o in out.iter_mut() {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

/// A synth ready to be put on a track ([`Mixer::prepare`], [`Mixer::put`]).
pub struct Prepared(Slot);

/// One synth the user can pick.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SynthInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// "vst3" or "built-in".
    pub kind: &'static str,
}

/// The synths on this PC: VST3 instruments (scanned from the standard folders) and Solara's
/// built-in instruments.
pub fn synths() -> Vec<SynthInfo> {
    let mut out: Vec<SynthInfo> = soundcraft_vst3_host::scan()
        .into_iter()
        .filter(|d| d.is_instrument)
        .map(|d| SynthInfo {
            id: d.id,
            name: d.name,
            vendor: d.vendor,
            kind: "vst3",
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out.extend(builtin_synths());
    out
}

/// Solara's built-in instruments (always there, no scan).
pub fn builtin_synths() -> Vec<SynthInfo> {
    soundcraft_dsp::plugins()
        .iter()
        .filter(|p| {
            p.is_instrument && !soundcraft_dsp::is_generator(p.id) && !p.id.contains("sampler")
        }) // the Sampler needs a recording
        .map(|p| SynthInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            vendor: "Solara".into(),
            kind: "built-in",
        })
        .collect()
}

/// Makes a synth by id (VST3 or built-in), not yet prepared for the device.
pub fn create_synth(id: &str) -> Result<Box<dyn Plugin>, String> {
    if id.starts_with(soundcraft_vst3_host::ID_PREFIX) {
        return soundcraft_vst3_host::instantiate(id).map_err(|e| e.to_string());
    }
    match soundcraft_dsp::plugin_info(id) {
        Some(info) if info.is_instrument => {
            soundcraft_dsp::create(id).ok_or_else(|| format!("cannot create {id}"))
        }
        Some(_) => Err(format!("{id} is an effect, not a synth")),
        None => Err(format!("no synth {id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(input: Option<&str>, channel: Option<u8>) -> TrackCfg {
        TrackCfg {
            input: input.map(String::from),
            channel,
            ..Default::default()
        }
    }

    #[test]
    fn parses_notes_and_controls() {
        assert_eq!(
            parse(&[0x90, 60, 100]),
            Some((
                1,
                Event::On {
                    note: 60,
                    velocity: 100
                }
            ))
        );
        assert_eq!(
            parse(&[0x92, 60, 0]),
            Some((3, Event::Off { note: 60 })),
            "note on with velocity 0 is a note off"
        );
        assert_eq!(parse(&[0x8F, 61, 40]), Some((16, Event::Off { note: 61 })));
        assert_eq!(parse(&[0xB0, 123, 0]), Some((1, Event::AllOff)));
        assert_eq!(
            parse(&[0xB0, 64, 127]),
            Some((1, Event::Other([0xB0, 64, 127]))),
            "sustain pedal passes through"
        );
        assert_eq!(parse(&[0xC1, 5]), Some((2, Event::Other([0xC1, 5, 0]))));
        assert_eq!(parse(&[0xF8]), None, "clock");
        assert_eq!(parse(&[0x90, 60]), None, "cut short");
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x40, 1, 2]), None, "not a status byte");
    }

    #[test]
    fn each_track_listens_to_its_own_input() {
        let tracks = vec![
            t(Some("Keys A"), None),
            t(Some("Keys B"), Some(2)),
            t(None, None),
            t(Some("Keys A"), Some(10)),
        ];
        assert_eq!(route(&tracks, "Keys A", 1), vec![0, 2]);
        assert_eq!(route(&tracks, "Keys B", 2), vec![1, 2]);
        assert_eq!(route(&tracks, "Keys B", 3), vec![2]);
        assert_eq!(route(&tracks, "Keys A", 10), vec![0, 2, 3]);
        let five = vec![t(None, None); 5];
        assert_eq!(
            route(&five, "x", 1).len(),
            MAX_TRACKS,
            "never more than four tracks"
        );
    }

    #[test]
    fn gain_is_clamped() {
        assert_eq!(
            TrackCfg {
                volume: Some(2.0),
                ..Default::default()
            }
            .gain(),
            1.0
        );
        assert_eq!(
            TrackCfg {
                volume: Some(f32::NAN),
                ..Default::default()
            }
            .gain(),
            0.8
        );
        assert_eq!(TrackCfg::default().gain(), 0.8);
    }

    #[test]
    fn a_builtin_synth_sounds_when_a_key_is_played() {
        let list = builtin_synths();
        assert!(!list.is_empty(), "Solara has built-in synths");
        let Some(s) = list.into_iter().next() else {
            return;
        };
        let mut m = Mixer::new(48_000.0);
        let synth = create_synth(&s.id).ok();
        assert!(
            synth.is_some(),
            "the built-in synth {} can be created",
            s.id
        );
        m.set(
            0,
            synth,
            &TrackCfg {
                volume: Some(1.0),
                ..Default::default()
            },
        );
        let mut out = vec![0.0f32; 1024 * 2];
        m.render(&mut out, 2);
        let quiet: f32 = out.iter().map(|x| x.abs()).sum();
        m.event(
            0,
            Event::On {
                note: 60,
                velocity: 110,
            },
        );
        m.render(&mut out, 2);
        let loud: f32 = out.iter().map(|x| x.abs()).sum();
        assert!(loud > quiet + 0.1, "{}: {quiet} -> {loud}", s.id);
        m.level(0, 1.0, true);
        m.render(&mut out, 2);
        assert!(out.iter().all(|x| *x == 0.0), "muted");
        m.all_off();
    }

    #[test]
    fn effects_are_not_synths() {
        assert!(create_synth("no-such-thing").is_err());
        assert!(
            builtin_synths()
                .iter()
                .all(|s| soundcraft_dsp::plugin_info(&s.id).is_some_and(|i| i.is_instrument))
        );
    }
}
