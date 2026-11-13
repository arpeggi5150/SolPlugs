//! The devices on Windows: MIDI inputs (WinMM, through `midir`) and the audio output (WASAPI,
//! through `cpal`). The audio callback never blocks or allocates: the MIDI threads and the
//! control side talk to it through lock-free queues, and what it lets go of comes back through a
//! queue to be dropped elsewhere.

use crate::{Event, MAX_TRACKS, Mixer, Prepared, TrackCfg, create_synth, parse, route};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;

/// What the control side sends to the audio thread.
enum Cmd {
    Event(usize, Event),
    Level(usize, f32, bool),
    Put(usize, Option<Prepared>),
    Inputs(Vec<rtrb::Consumer<(usize, Event)>>),
    AllOff,
}

/// What the audio thread hands back to be dropped off it.
enum Trash {
    Slot(Prepared),
    Inputs(Vec<rtrb::Consumer<(usize, Event)>>),
}

/// A track's state as the program reports it.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct TrackState {
    pub synth: Option<String>,
    pub error: Option<String>,
}

/// Activity seen on an input (for the module's lights).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Activity {
    pub track: usize,
    pub note: u8,
    pub on: bool,
}

/// The running rig: the audio stream, the MIDI connections and the synths.
pub struct Rig {
    _stream: cpal::Stream,
    cmd: rtrb::Producer<Cmd>,
    trash: rtrb::Consumer<Trash>,
    inputs: Vec<midir::MidiInputConnection<()>>,
    tracks: Vec<TrackCfg>,
    states: Vec<TrackState>,
    rate: f32,
    pub device: String,
    pub buffer_frames: u32,
    activity: mpsc::Sender<Activity>,
}

/// The MIDI inputs on this PC, by name.
pub fn midi_inputs() -> Vec<String> {
    let Ok(m) = midir::MidiInput::new("SolPlugs MIDI Keys (list)") else {
        return Vec::new();
    };
    m.ports()
        .iter()
        .filter_map(|p| m.port_name(p).ok())
        .collect()
}

/// The default audio output's name.
pub fn output_name() -> Option<String> {
    let d = cpal::default_host().default_output_device()?;
    d.description().ok().map(|x| x.name().to_owned())
}

impl Rig {
    /// Opens the default audio output with a small buffer (`want_frames`, else the device's
    /// default) and starts silent. `activity` gets each note played.
    pub fn open(want_frames: u32, activity: mpsc::Sender<Activity>) -> Result<Rig, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No audio output device.")?;
        let name = device
            .description()
            .ok()
            .map(|x| x.name().to_owned())
            .unwrap_or_else(|| "Audio output".into());
        let def = device
            .default_output_config()
            .map_err(|e| format!("The audio output cannot be opened: {e}"))?;
        if def.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "The audio output uses {:?} samples; MIDI Keys plays to 32-bit float outputs (the Windows default).",
                def.sample_format()
            ));
        }
        let mut config = def.config();
        let buffer = match def.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } if (16..=4096).contains(&want_frames) => {
                let n = want_frames.clamp(*min, *max);
                config.buffer_size = cpal::BufferSize::Fixed(n);
                n
            }
            _ => 0,
        };
        let channels = usize::from(config.channels.max(1));
        let rate = config.sample_rate as f32;
        let (cmd_tx, mut cmd_rx) = rtrb::RingBuffer::<Cmd>::new(1024);
        let (mut trash_tx, trash_rx) = rtrb::RingBuffer::<Trash>::new(64);
        let mut mixer = Mixer::new(rate);
        let mut inputs: Vec<rtrb::Consumer<(usize, Event)>> = Vec::with_capacity(MAX_TRACKS + 1);
        let data = move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
            while let Ok(c) = cmd_rx.pop() {
                match c {
                    Cmd::Event(t, e) => mixer.event(t, e),
                    Cmd::Level(t, g, m) => mixer.level(t, g, m),
                    Cmd::Put(t, slot) => {
                        if let Some(old) = mixer.put(t, slot) {
                            let _ = trash_tx.push(Trash::Slot(old));
                        }
                    }
                    Cmd::Inputs(next) => {
                        let old = std::mem::replace(&mut inputs, next);
                        let _ = trash_tx.push(Trash::Inputs(old));
                    }
                    Cmd::AllOff => mixer.all_off(),
                }
            }
            for q in inputs.iter_mut() {
                while let Ok((t, e)) = q.pop() {
                    mixer.event(t, e);
                }
            }
            mixer.render(out, channels);
        };
        let stream = device
            .build_output_stream(
                config,
                data,
                move |e: cpal::Error| log::warn!("audio stream: {e}"),
                None,
            )
            .map_err(|e| format!("The audio output cannot be opened: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("The audio output cannot start: {e}"))?;
        Ok(Rig {
            _stream: stream,
            cmd: cmd_tx,
            trash: trash_rx,
            inputs: Vec::new(),
            tracks: Vec::new(),
            states: vec![TrackState::default(); MAX_TRACKS],
            rate,
            device: name,
            buffer_frames: buffer,
            activity,
        })
    }

    /// The output latency of one buffer, in milliseconds (0 when the device chose the size).
    pub fn latency_ms(&self) -> f32 {
        if self.buffer_frames == 0 {
            0.0
        } else {
            self.buffer_frames as f32 * 1000.0 / self.rate
        }
    }

    /// Applies the tracks: new synths are made and prepared here (off the audio thread), the
    /// unchanged ones only get their volume and mute, and the MIDI inputs are connected again.
    pub fn apply(&mut self, tracks: &[TrackCfg]) -> Vec<TrackState> {
        self.drain();
        let mixer_rate = Mixer::new(self.rate);
        for i in 0..MAX_TRACKS {
            let want = tracks.get(i).cloned().unwrap_or_default();
            let want_synth = want.synth.clone().filter(|s| !s.is_empty());
            let had = self.states.get(i).and_then(|s| s.synth.clone());
            if want_synth == had && self.states.get(i).is_some_and(|s| s.error.is_none()) {
                let _ = self.cmd.push(Cmd::Level(i, want.gain(), want.mute));
                continue;
            }
            let state = match want_synth.as_deref() {
                None => {
                    let _ = self.cmd.push(Cmd::Put(i, None));
                    TrackState::default()
                }
                Some(id) => match create_synth(id) {
                    Ok(s) => {
                        let _ = self
                            .cmd
                            .push(Cmd::Put(i, Some(mixer_rate.prepare(s, &want))));
                        TrackState {
                            synth: Some(id.to_string()),
                            error: None,
                        }
                    }
                    Err(e) => {
                        let _ = self.cmd.push(Cmd::Put(i, None));
                        TrackState {
                            synth: Some(id.to_string()),
                            error: Some(e),
                        }
                    }
                },
            };
            if let Some(s) = self.states.get_mut(i) {
                *s = state;
            }
        }
        self.tracks = tracks.iter().take(MAX_TRACKS).cloned().collect();
        self.connect();
        self.states.clone()
    }

    /// Connects every MIDI input a track listens to (all of them when a track takes any input).
    fn connect(&mut self) {
        self.inputs.clear(); // closes the old connections first (their queues are replaced below)
        let names = midi_inputs();
        let any = self
            .tracks
            .iter()
            .any(|t| t.input.as_deref().is_none_or(str::is_empty) && t.synth.is_some());
        let wanted: Vec<String> = names
            .into_iter()
            .filter(|n| {
                any || self
                    .tracks
                    .iter()
                    .any(|t| t.input.as_deref() == Some(n.as_str()))
            })
            .take(16)
            .collect();
        let mut queues = Vec::new();
        for name in wanted {
            let Ok(m) = midir::MidiInput::new("SolPlugs MIDI Keys") else {
                continue;
            };
            let Some(port) = m
                .ports()
                .into_iter()
                .find(|p| m.port_name(p).ok().as_deref() == Some(name.as_str()))
            else {
                continue;
            };
            let (mut tx, rx) = rtrb::RingBuffer::<(usize, Event)>::new(512);
            let tracks = self.tracks.clone();
            let seen = self.activity.clone();
            let input = name.clone();
            let cb = move |_stamp: u64, bytes: &[u8], _: &mut ()| {
                let Some((channel, ev)) = parse(bytes) else {
                    return;
                };
                for t in route(&tracks, &input, channel) {
                    let _ = tx.push((t, ev));
                    match ev {
                        Event::On { note, .. } => {
                            let _ = seen.send(Activity {
                                track: t,
                                note,
                                on: true,
                            });
                        }
                        Event::Off { note } => {
                            let _ = seen.send(Activity {
                                track: t,
                                note,
                                on: false,
                            });
                        }
                        _ => {}
                    }
                }
            };
            match m.connect(&port, "solplug-midi-keys", cb, ()) {
                Ok(c) => {
                    self.inputs.push(c);
                    queues.push(rx);
                }
                Err(e) => log::warn!("MIDI input {name}: {e}"),
            }
        }
        let _ = self.cmd.push(Cmd::Inputs(queues));
    }

    /// The MIDI inputs connected now.
    pub fn connected(&self) -> usize {
        self.inputs.len()
    }

    /// A note from the screen (Command Center's test keys).
    pub fn note(&mut self, track: usize, note: u8, velocity: u8, on: bool) {
        let ev = if on && velocity > 0 {
            Event::On {
                note: note & 0x7F,
                velocity: velocity & 0x7F,
            }
        } else {
            Event::Off { note: note & 0x7F }
        };
        let _ = self.cmd.push(Cmd::Event(track.min(MAX_TRACKS - 1), ev));
    }

    /// Silences everything.
    pub fn panic(&mut self) {
        let _ = self.cmd.push(Cmd::AllOff);
    }

    /// Drops what the audio thread let go of.
    pub fn drain(&mut self) {
        while let Ok(t) = self.trash.pop() {
            match t {
                Trash::Slot(s) => drop(s),
                Trash::Inputs(q) => drop(q),
            }
        }
    }
}
