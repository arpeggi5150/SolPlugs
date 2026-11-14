//! The engine: parameters, meters, the audio-thread [`Processor`], graph-mode capture and
//! the background worker that analyses and renders takes.
//!
//! Threads:
//! - audio thread: [`Processor::process`] (no locks, no allocation);
//! - UI thread: reads meters and the trail, edits notes under the graph lock;
//! - worker thread: assembles captured audio into a take, analyses it into notes and
//!   renders edits; publishes the rendered take through an `ArcSwap` the audio thread reads.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use serde::{Deserialize, Serialize};
use solplugs_pitch_core::correct::CorrectParams;
use solplugs_pitch_core::notes::{Analysis, Formant, NoteEdits, analyse, render};
use solplugs_pitch_core::realtime::{self, Config, FrameInfo, Range, Realtime};
use solplugs_pitch_core::scale::{Scale, ScaleKind};

use crate::params::{COUNT, P, Values};

/// Longest take, seconds.
pub const MAX_TAKE_S: f64 = 600.0;
/// Capture chunk size, samples.
const CHUNK: usize = 256;

/// A change the UI made that the host must hear about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostEvent {
    /// Gesture start.
    Begin(P),
    /// New plain value.
    Value(P, f64),
    /// Gesture end.
    End(P),
}

/// Where the host transport is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    /// Playing (or recording).
    pub playing: bool,
    /// Timeline position of the block's first sample, samples, if the host says.
    pub pos: Option<i64>,
}

/// One point of the pitch trail shown in automatic mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrailPoint {
    /// Input pitch (MIDI), 0 when unvoiced.
    pub input: f32,
    /// Output pitch (MIDI), 0 when unvoiced.
    pub output: f32,
}

/// Graph-mode status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphStatus {
    /// No take.
    #[default]
    Empty,
    /// Capture armed, waiting for the transport.
    Armed,
    /// Recording what plays through the track.
    Capturing,
    /// Detecting notes.
    Analysing,
    /// Notes ready to edit.
    Ready,
}

/// A captured take.
#[derive(Clone, Debug, Default)]
pub struct Take {
    /// Sample rate it was captured at.
    pub sample_rate: f32,
    /// Timeline position of its first sample.
    pub start: i64,
    /// Audio, one Vec per channel.
    pub channels: Vec<Vec<f32>>,
}

impl Take {
    /// Length, samples.
    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Mono mix.
    pub fn mono(&self) -> Vec<f32> {
        let n = self.channels.len().max(1) as f32;
        (0..self.len())
            .map(|i| self.channels.iter().map(|c| c[i]).sum::<f32>() / n)
            .collect()
    }
}

/// The rendered take the audio thread plays in graph mode.
#[derive(Debug)]
pub struct Rendered {
    /// Timeline position of the first sample.
    pub start: i64,
    /// Sample rate.
    pub sample_rate: f32,
    /// Audio per channel.
    pub channels: Vec<Vec<f32>>,
}

/// Graph-mode state shared by the UI and the worker (under a mutex; never touched by the
/// audio thread).
#[derive(Default)]
pub struct Graph {
    /// Status.
    pub status: GraphStatus,
    /// The take.
    pub take: Option<Arc<Take>>,
    /// Its analysis.
    pub analysis: Option<Arc<Analysis>>,
    /// Notes and edits.
    pub edits: NoteEdits,
    /// Undo stack.
    pub undo: Vec<NoteEdits>,
    /// Redo stack.
    pub redo: Vec<NoteEdits>,
    /// Bumped on every edit; the worker renders when it changes.
    pub version: u64,
    /// Last problem to show.
    pub message: Option<String>,
}

impl Graph {
    /// Records the current edits for undo, before changing them.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.edits.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Marks the edits changed (the worker re-renders).
    pub fn touch(&mut self) {
        self.version += 1;
    }

    /// Undo.
    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edits, prev));
            self.touch();
        }
    }

    /// Redo.
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.edits, next));
            self.touch();
        }
    }
}

/// Live readings for the UI.
#[derive(Default)]
pub struct Meters {
    input: AtomicU32,
    output: AtomicU32,
    note: AtomicU32,
    clarity: AtomicU32,
    level: AtomicU32,
}

fn load_f(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store_f(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

impl Meters {
    /// Detected input pitch (MIDI), 0 when unvoiced.
    pub fn input(&self) -> f32 {
        load_f(&self.input)
    }
    /// Output pitch (MIDI), 0 when unvoiced.
    pub fn output(&self) -> f32 {
        load_f(&self.output)
    }
    /// Target note, or -1.
    pub fn note(&self) -> i32 {
        load_f(&self.note) as i32
    }
    /// Detector clarity.
    pub fn clarity(&self) -> f32 {
        load_f(&self.clarity)
    }
    /// Input RMS (linear).
    pub fn level(&self) -> f32 {
        load_f(&self.level)
    }
}

struct Shared {
    params: Values,
    custom_mask: AtomicU32,
    meters: Meters,
    latency: AtomicU32,
    sample_rate: AtomicU32,
    playhead: AtomicI64,
    playing: AtomicBool,
    armed: AtomicBool,
    capturing: AtomicBool,
    capture_overflow: AtomicBool,
    graph: Mutex<Graph>,
    rendered: ArcSwapOption<Rendered>,
    listener: Mutex<Option<Arc<dyn Fn(HostEvent) + Send + Sync>>>,
    trail: Mutex<Option<rtrb::Consumer<TrailPoint>>>,
    stop: AtomicBool,
    latency_changed: AtomicBool,
    graph_epoch: AtomicU64,
}

/// Messages to the worker.
enum ToWorker {
    /// A new processor's capture stream.
    Stream(rtrb::Consumer<Chunk>),
    /// Analyse this take (loaded from state).
    Load(Take, NoteEdits),
}

#[derive(Clone, Copy)]
struct Chunk {
    pos: i64,
    len: u16,
    channels: u8,
    data: [[f32; CHUNK]; 2],
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            pos: 0,
            len: 0,
            channels: 0,
            data: [[0.0; CHUNK]; 2],
        }
    }
}

/// The engine, shared by the plugin glue, the editor and Solara.
pub struct Engine {
    shared: Arc<Shared>,
    to_worker: Mutex<mpsc::Sender<ToWorker>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// A new engine with default parameters and its worker thread.
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            params: Values::default(),
            custom_mask: AtomicU32::new(0xfff),
            meters: Meters::default(),
            latency: AtomicU32::new(0),
            sample_rate: AtomicU32::new(48_000f32.to_bits()),
            playhead: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            capture_overflow: AtomicBool::new(false),
            graph: Mutex::new(Graph::default()),
            rendered: ArcSwapOption::empty(),
            listener: Mutex::new(None),
            trail: Mutex::new(None),
            stop: AtomicBool::new(false),
            latency_changed: AtomicBool::new(false),
            graph_epoch: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::channel();
        let s = shared.clone();
        let worker = std::thread::Builder::new()
            .name("solplugs-pitch-worker".into())
            .spawn(move || Worker::new(s, rx).run())
            .map_err(|e| log::error!("SolPlugs Pitch: couldn't start the worker: {e}"))
            .ok();
        Self {
            shared,
            to_worker: Mutex::new(tx),
            worker,
        }
    }

    /// Parameter values.
    pub fn params(&self) -> &Values {
        &self.shared.params
    }

    /// Meters.
    pub fn meters(&self) -> &Meters {
        &self.shared.meters
    }

    /// Sets a parameter from the host (no echo back to the host).
    pub fn set_from_host(&self, p: P, plain: f64) {
        self.shared.params.set(p, plain);
    }

    /// Who hears about UI edits (the plugin glue). `None` to detach.
    pub fn set_listener(&self, f: Option<Arc<dyn Fn(HostEvent) + Send + Sync>>) {
        if let Ok(mut l) = self.shared.listener.lock() {
            *l = f;
        }
    }

    fn notify(&self, e: HostEvent) {
        let l = self.shared.listener.lock().ok().and_then(|l| l.clone());
        if let Some(l) = l {
            l(e);
        }
    }

    /// UI: start of a gesture on `p`.
    pub fn begin_edit(&self, p: P) {
        self.notify(HostEvent::Begin(p));
    }

    /// UI: new value for `p` (inside a gesture, or a one-off click).
    pub fn edit(&self, p: P, plain: f64) {
        let v = self.shared.params.set(p, plain);
        self.notify(HostEvent::Value(p, v));
    }

    /// UI: end of a gesture on `p`.
    pub fn end_edit(&self, p: P) {
        self.notify(HostEvent::End(p));
    }

    /// UI: a whole click (begin, value, end).
    pub fn set(&self, p: P, plain: f64) {
        self.begin_edit(p);
        self.edit(p, plain);
        self.end_edit(p);
    }

    /// Custom scale notes, bit 0 = C.
    pub fn custom_mask(&self) -> u16 {
        self.shared.custom_mask.load(Ordering::Relaxed) as u16
    }

    /// Sets the custom scale notes.
    pub fn set_custom_mask(&self, m: u16) {
        self.shared
            .custom_mask
            .store(u32::from(m & 0xfff), Ordering::Relaxed);
    }

    /// The scale the parameters describe.
    pub fn scale(&self) -> Scale {
        scale_of(&self.shared)
    }

    /// Processing latency, samples (for the host).
    pub fn latency(&self) -> u32 {
        // Before the first activation, report what the current range will need, so a
        // host that asks early still compensates.
        match self.shared.latency.load(Ordering::Relaxed) {
            0 => self.latency_for(Range::from_index(self.shared.params.index(P::Range))),
            n => n,
        }
    }

    /// Latency for `range` at the current sample rate, samples.
    pub fn latency_for(&self, range: Range) -> u32 {
        Config {
            sample_rate: self.sample_rate(),
            range,
        }
        .latency() as u32
    }

    /// True once after the latency changed (the glue tells the host).
    pub fn take_latency_changed(&self) -> bool {
        self.shared.latency_changed.swap(false, Ordering::AcqRel)
    }

    /// Tells the engine the host's sample rate before activation, so [`Engine::latency`]
    /// is right when a host asks for it early.
    pub fn set_sample_rate(&self, sample_rate: f32) {
        if sample_rate.is_finite() && sample_rate > 0.0 {
            self.shared
                .sample_rate
                .store(sample_rate.to_bits(), Ordering::Relaxed);
        }
    }

    /// Sample rate of the running processor (or the host's, before activation).
    pub fn sample_rate(&self) -> f32 {
        load_f(&self.shared.sample_rate)
    }

    /// Host timeline position, samples, and whether playing.
    pub fn playhead(&self) -> (i64, bool) {
        (
            self.shared.playhead.load(Ordering::Relaxed),
            self.shared.playing.load(Ordering::Relaxed),
        )
    }

    /// Graph state (UI thread).
    pub fn graph(&self) -> std::sync::MutexGuard<'_, Graph> {
        match self.shared.graph.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Arms or disarms capture.
    pub fn arm_capture(&self, on: bool) {
        self.shared.armed.store(on, Ordering::Relaxed);
        let mut g = self.graph();
        if on && g.status != GraphStatus::Capturing {
            g.status = GraphStatus::Armed;
        } else if !on && g.status == GraphStatus::Armed {
            g.status = if g.take.is_some() {
                GraphStatus::Ready
            } else {
                GraphStatus::Empty
            };
        }
    }

    /// Whether capture is armed.
    pub fn armed(&self) -> bool {
        self.shared.armed.load(Ordering::Relaxed)
    }

    /// Drops the take and its edits.
    pub fn clear_take(&self) {
        self.shared.rendered.store(None);
        let mut g = self.graph();
        *g = Graph::default();
        self.shared.graph_epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Drains the pitch trail (UI thread).
    pub fn drain_trail(&self, mut f: impl FnMut(TrailPoint)) {
        if let Ok(mut t) = self.shared.trail.lock()
            && let Some(rx) = t.as_mut()
        {
            while let Ok(p) = rx.pop() {
                f(p);
            }
        }
    }

    /// A processor for the audio thread (one per activation).
    pub fn processor(self: &Arc<Self>, sample_rate: f64, max_block: usize) -> Processor {
        let (trail_tx, trail_rx) = rtrb::RingBuffer::new(4096);
        if let Ok(mut t) = self.shared.trail.lock() {
            *t = Some(trail_rx);
        }
        let (cap_tx, cap_rx) = rtrb::RingBuffer::new(1024);
        if let Ok(tx) = self.to_worker.lock() {
            let _ = tx.send(ToWorker::Stream(cap_rx));
        }
        Processor::new(
            self.shared.clone(),
            sample_rate as f32,
            max_block,
            trail_tx,
            cap_tx,
        )
    }

    /// Serialises parameters, the custom scale, the take and its edits.
    pub fn save_state(&self) -> Vec<u8> {
        let g = self.graph();
        let take = g.take.clone();
        let state = State {
            params: P::ALL
                .iter()
                .map(|p| (p.info().name.to_string(), self.shared.params.get(*p)))
                .collect(),
            custom_mask: self.custom_mask(),
            take: take.as_ref().map(|t| TakeMeta {
                sample_rate: t.sample_rate,
                start: t.start,
                channels: t.channels.len(),
                len: t.len(),
            }),
            edits: take.as_ref().map(|_| g.edits.clone()),
        };
        drop(g);
        let json = serde_json::to_vec(&state).unwrap_or_default();
        let mut out = Vec::with_capacity(
            12 + json.len() + take.as_ref().map_or(0, |t| t.len() * t.channels.len() * 4),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        if let Some(t) = take {
            for c in &t.channels {
                for s in c {
                    out.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
        out
    }

    /// Restores what [`Engine::save_state`] wrote. Returns false, changing nothing, for
    /// data that isn't a SolPlugs Pitch state (empty, damaged or from something else).
    pub fn load_state(&self, data: &[u8]) -> bool {
        let Some(rest) = data.strip_prefix(MAGIC.as_slice()) else {
            return false;
        };
        if rest.len() < 4 {
            return false;
        }
        let n = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let Some(json) = rest.get(4..4 + n) else {
            return false;
        };
        let Ok(state) = serde_json::from_slice::<State>(json) else {
            return false;
        };
        for (name, v) in &state.params {
            if let Some(p) = P::ALL.iter().find(|p| p.info().name == name) {
                self.shared.params.set(*p, *v);
            }
        }
        self.set_custom_mask(state.custom_mask);
        self.clear_take();
        let (Some(meta), Some(edits)) = (state.take, state.edits) else {
            return true;
        };
        let audio = &rest[4 + n..];
        let need = meta.len * meta.channels * 4;
        if meta.channels == 0 || meta.channels > 2 || audio.len() < need {
            // Settings restored; the take is damaged, so it is dropped.
            return true;
        }
        let channels = (0..meta.channels)
            .map(|c| {
                audio[c * meta.len * 4..(c + 1) * meta.len * 4]
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect()
            })
            .collect();
        let take = Take {
            sample_rate: meta.sample_rate,
            start: meta.start,
            channels,
        };
        self.graph().status = GraphStatus::Analysing;
        if let Ok(tx) = self.to_worker.lock() {
            let _ = tx.send(ToWorker::Load(take, edits));
        }
        true
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

const MAGIC: &[u8; 8] = b"SPPITCH1";

#[derive(Serialize, Deserialize)]
struct TakeMeta {
    sample_rate: f32,
    start: i64,
    channels: usize,
    len: usize,
}

#[derive(Serialize, Deserialize)]
struct State {
    params: Vec<(String, f64)>,
    custom_mask: u16,
    take: Option<TakeMeta>,
    edits: Option<NoteEdits>,
}

fn scale_of(s: &Shared) -> Scale {
    let kind = ScaleKind::from_index(s.params.index(P::Scale));
    Scale::new(
        (s.params.index(P::Key) % 12) as u8,
        kind,
        s.custom_mask.load(Ordering::Relaxed) as u16,
    )
}

/// The audio-thread half. Created per activation; allocation-free while processing.
pub struct Processor {
    shared: Arc<Shared>,
    rts: Vec<Realtime>,
    range: usize,
    trail: rtrb::Producer<TrailPoint>,
    capture: rtrb::Producer<Chunk>,
    chunk: Chunk,
    sample_rate: f32,
    /// Graph playback gain (0 = live, 1 = rendered take), ramped.
    take_gain: f32,
    ramp: f32,
}

impl Processor {
    fn new(
        shared: Arc<Shared>,
        sample_rate: f32,
        _max_block: usize,
        trail: rtrb::Producer<TrailPoint>,
        capture: rtrb::Producer<Chunk>,
    ) -> Self {
        let rts: Vec<Realtime> = Range::ALL
            .iter()
            .map(|&range| Realtime::new(Config { sample_rate, range }))
            .collect();
        let range = shared.params.index(P::Range).min(2);
        store_f(&shared.sample_rate, sample_rate);
        shared
            .latency
            .store(rts[range].latency() as u32, Ordering::Relaxed);
        Self {
            shared,
            rts,
            range,
            trail,
            capture,
            chunk: Chunk::default(),
            sample_rate,
            take_gain: 0.0,
            ramp: 1.0 / (0.005 * sample_rate).max(1.0),
        }
    }

    /// Latency now, samples.
    pub fn latency(&self) -> usize {
        self.rts[self.range].latency()
    }

    fn live_params(&self) -> realtime::Params {
        let p = &self.shared.params;
        let graph = p.on(P::Mode);
        realtime::Params {
            correct: CorrectParams {
                scale: scale_of(&self.shared),
                retune_ms: p.get(P::Retune) as f32,
                humanize: p.get(P::Humanize) as f32 / 100.0,
                // Graph mode: the live path is transparent; the take carries the edits.
                amount: if graph {
                    0.0
                } else {
                    p.get(P::Amount) as f32 / 100.0
                },
                vibrato: if graph {
                    1.0
                } else {
                    p.get(P::Vibrato) as f32 / 100.0
                },
            },
            formant_preserve: p.on(P::Formant),
            formant_shift: p.get(P::FormantShift) as f32,
            bypass: p.on(P::Bypass),
            transpose: if graph {
                0.0
            } else {
                p.get(P::Transpose) as f32
            },
            ..realtime::Params::default()
        }
    }

    /// Processes one block in place. `channels` are 1 or 2 equal-length buffers.
    pub fn process(&mut self, channels: &mut [&mut [f32]], transport: Transport) {
        let n = channels.first().map_or(0, |c| c.len());
        if n == 0 {
            return;
        }
        let range = self.shared.params.index(P::Range).min(2);
        if range != self.range {
            self.range = range;
            self.rts[range].reset();
            self.shared
                .latency
                .store(self.rts[range].latency() as u32, Ordering::Relaxed);
            self.shared.latency_changed.store(true, Ordering::Release);
        }
        if let Some(pos) = transport.pos {
            self.shared.playhead.store(pos, Ordering::Relaxed);
        }
        self.shared
            .playing
            .store(transport.playing, Ordering::Relaxed);
        self.capture_block(channels, transport);

        let params = self.live_params();
        let meters = &self.shared.meters;
        let trail = &mut self.trail;
        self.rts[self.range].process(channels, &params, &mut |f: &FrameInfo| {
            store_f(&meters.input, f.corrected.input);
            store_f(&meters.output, f.corrected.output);
            store_f(&meters.note, f.corrected.note as f32);
            store_f(&meters.clarity, f.clarity);
            store_f(&meters.level, f.rms);
            let _ = trail.push(TrailPoint {
                input: f.corrected.input,
                output: if f.corrected.input > 0.0 {
                    f.corrected.input + f.corrected.shift + params.transpose
                } else {
                    0.0
                },
            });
        });
        self.play_take(channels, transport, params.bypass);
    }

    fn capture_block(&mut self, channels: &[&mut [f32]], transport: Transport) {
        let s = &self.shared;
        let want = s.armed.load(Ordering::Relaxed)
            && transport.playing
            && transport.pos.is_some()
            && s.params.on(P::Mode);
        s.capturing.store(want, Ordering::Relaxed);
        let Some(pos) = transport.pos.filter(|_| want) else {
            return;
        };
        let n = channels[0].len();
        let nch = channels.len().min(2);
        let mut off = 0;
        while off < n {
            let k = (n - off).min(CHUNK);
            self.chunk.pos = pos + off as i64;
            self.chunk.len = k as u16;
            self.chunk.channels = nch as u8;
            for c in 0..nch {
                self.chunk.data[c][..k].copy_from_slice(&channels[c][off..off + k]);
            }
            if self.capture.push(self.chunk).is_err() {
                s.capture_overflow.store(true, Ordering::Relaxed);
            }
            off += k;
        }
    }

    fn play_take(&mut self, channels: &mut [&mut [f32]], transport: Transport, bypass: bool) {
        let graph = self.shared.params.on(P::Mode);
        let guard = self.shared.rendered.load();
        let take = guard
            .as_ref()
            .filter(|r| r.sample_rate == self.sample_rate && !r.channels.is_empty());
        let active =
            graph && !bypass && transport.playing && !self.shared.capturing.load(Ordering::Relaxed);
        let (Some(take), Some(pos)) = (take, transport.pos.filter(|_| active)) else {
            // Fade out what was playing (it is gone; only the gain matters).
            self.take_gain = 0.0;
            return;
        };
        let lat = self.latency() as i64;
        let len = take.channels[0].len() as i64;
        let n = channels[0].len();
        for i in 0..n {
            // The host delays everything else by our latency, so this output sample sits
            // at timeline position pos + i - latency.
            let t = pos + i as i64 - lat - take.start;
            let inside = t >= 0 && t < len;
            let target = if inside { 1.0 } else { 0.0 };
            if self.take_gain < target {
                self.take_gain = (self.take_gain + self.ramp).min(1.0);
            } else if self.take_gain > target {
                self.take_gain = (self.take_gain - self.ramp).max(0.0);
            }
            if self.take_gain <= 0.0 {
                continue;
            }
            let ti = t.clamp(0, len - 1) as usize;
            for (c, ch) in channels.iter_mut().enumerate() {
                let src = &take.channels[c.min(take.channels.len() - 1)];
                let live = ch[i];
                ch[i] = live + self.take_gain * (src[ti] - live);
            }
        }
    }
}

/// The background worker.
struct Worker {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<ToWorker>,
    stream: Option<rtrb::Consumer<Chunk>>,
    building: Option<Take>,
    last_chunk: Instant,
    rendered_version: u64,
    rendered_epoch: u64,
    dirty_since: Option<Instant>,
    graveyard: Vec<Arc<Rendered>>,
}

impl Worker {
    fn new(shared: Arc<Shared>, rx: mpsc::Receiver<ToWorker>) -> Self {
        Self {
            shared,
            rx,
            stream: None,
            building: None,
            last_chunk: Instant::now(),
            rendered_version: 0,
            rendered_epoch: 0,
            dirty_since: None,
            graveyard: Vec::new(),
        }
    }

    fn run(mut self) {
        while !self.shared.stop.load(Ordering::Relaxed) {
            match self.rx.recv_timeout(Duration::from_millis(10)) {
                Ok(ToWorker::Stream(c)) => {
                    // A new activation: keep what the old processor already captured.
                    self.drain_capture();
                    self.stream = Some(c);
                }
                Ok(ToWorker::Load(take, edits)) => self.finish(take, Some(edits)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.drain_capture();
            self.maybe_render();
        }
    }

    fn drain_capture(&mut self) {
        let Some(rx) = self.stream.as_mut() else {
            return;
        };
        let sr = load_f(&self.shared.sample_rate);
        let max = (MAX_TAKE_S * f64::from(sr)) as i64;
        let mut got = false;
        while let Ok(chunk) = rx.pop() {
            got = true;
            let take = self.building.get_or_insert_with(|| Take {
                sample_rate: sr,
                start: chunk.pos,
                channels: vec![Vec::new(); usize::from(chunk.channels.max(1))],
            });
            let mut start = chunk.pos - take.start;
            if start < 0 {
                // Captured before the take: grow it at the front.
                let grow = (-start) as usize;
                if (take.len() + grow) as i64 > max {
                    continue;
                }
                for c in take.channels.iter_mut() {
                    c.splice(0..0, std::iter::repeat_n(0.0, grow));
                }
                take.start = chunk.pos;
                start = 0;
            }
            let end = start + i64::from(chunk.len);
            if end > max {
                continue;
            }
            for (ci, c) in take.channels.iter_mut().enumerate() {
                if (c.len() as i64) < end {
                    c.resize(end as usize, 0.0);
                }
                let src = &chunk.data[ci.min(usize::from(chunk.channels.max(1)) - 1)];
                c[start as usize..end as usize].copy_from_slice(&src[..usize::from(chunk.len)]);
            }
        }
        let capturing = self.shared.capturing.load(Ordering::Relaxed);
        if got {
            self.last_chunk = Instant::now();
            if let Ok(mut g) = self.shared.graph.lock() {
                g.status = GraphStatus::Capturing;
            }
        } else if self.building.is_some()
            && !capturing
            && self.last_chunk.elapsed() > Duration::from_millis(200)
        {
            // Transport stopped: the take is complete.
            self.shared.armed.store(false, Ordering::Relaxed);
            if let Some(take) = self.building.take() {
                self.finish(take, None);
            }
        }
    }

    /// Analyses a finished take and publishes it.
    fn finish(&mut self, take: Take, edits: Option<NoteEdits>) {
        if let Ok(mut g) = self.shared.graph.lock() {
            g.status = GraphStatus::Analysing;
        }
        let range = Range::from_index(self.shared.params.index(P::Range));
        let an = analyse(&take.mono(), take.sample_rate, range);
        let edits = match edits {
            Some(e) if e.len == an.len => e,
            _ => NoteEdits::new(&an),
        };
        let take = Arc::new(take);
        let an = Arc::new(an);
        let epoch = self.shared.graph_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let version;
        {
            let mut g = match self.shared.graph.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            g.take = Some(take.clone());
            g.analysis = Some(an.clone());
            g.edits = edits.clone();
            g.undo.clear();
            g.redo.clear();
            g.version += 1;
            version = g.version;
            g.status = GraphStatus::Ready;
            g.message = self
                .shared
                .capture_overflow
                .swap(false, Ordering::Relaxed)
                .then(|| "Some audio was dropped while capturing (system too busy).".into());
        }
        self.publish(&take, &an, &edits);
        self.rendered_version = version;
        self.rendered_epoch = epoch;
    }

    fn maybe_render(&mut self) {
        let (version, epoch, job) = {
            let Ok(g) = self.shared.graph.lock() else {
                return;
            };
            let epoch = self.shared.graph_epoch.load(Ordering::Relaxed);
            if g.version == self.rendered_version && epoch == self.rendered_epoch {
                return;
            }
            let job = match (&g.take, &g.analysis) {
                (Some(t), Some(a)) => Some((t.clone(), a.clone(), g.edits.clone())),
                _ => None,
            };
            (g.version, epoch, job)
        };
        // Debounce drags: render 30 ms after the last change.
        let since = *self.dirty_since.get_or_insert_with(Instant::now);
        if since.elapsed() < Duration::from_millis(30) {
            return;
        }
        self.dirty_since = None;
        self.rendered_version = version;
        self.rendered_epoch = epoch;
        if let Some((take, an, edits)) = job {
            self.publish(&take, &an, &edits);
        }
    }

    fn publish(&mut self, take: &Take, an: &Analysis, edits: &NoteEdits) {
        let p = &self.shared.params;
        let formant = Formant {
            preserve: p.on(P::Formant),
            shift: p.get(P::FormantShift) as f32,
        };
        let channels = if edits.is_edited() {
            let refs: Vec<&[f32]> = take.channels.iter().map(Vec::as_slice).collect();
            render(&refs, an, edits, formant)
        } else {
            take.channels.clone()
        };
        let r = Arc::new(Rendered {
            start: take.start,
            sample_rate: take.sample_rate,
            channels,
        });
        // Keep the previous take alive a little so the audio thread never frees it.
        if let Some(old) = self.shared.rendered.swap(Some(r)) {
            self.graveyard.push(old);
            if self.graveyard.len() > 3 {
                self.graveyard.remove(0);
            }
        }
    }
}

/// Number of parameters (re-export for glue).
pub const PARAM_COUNT: usize = COUNT;
