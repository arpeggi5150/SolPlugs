//! The engine: parameters, meters, the audio-thread [`Processor`], presets, A/B, MATCH and
//! auto-makeup. Same shape as the compressor's and the EQ's engines.
//!
//! Threads:
//! - audio thread: [`Processor::process`] (no locks, no allocation);
//! - UI thread: reads meters, drains the waveform columns, edits parameters.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use solplugs_delay_core::util::lin_to_db;
use solplugs_delay_core::{CHUNK, Ctl, DelayFx, TapTempo, Version, latency_for};

use crate::params::{G, Pid, TABLE, Which};
use crate::presets::PRESETS;

/// A change the UI made that the host must hear about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostEvent {
    /// Gesture start.
    Begin(Pid),
    /// New plain value.
    Value(Pid, f64),
    /// Gesture end.
    End(Pid),
}

/// One display column (about 5 ms of audio): waveform extents per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Input (BEFORE) min and max, left and right.
    pub before: [(f32, f32); 2],
    /// Output (AFTER) min and max, left and right.
    pub after: [(f32, f32); 2],
}

impl Column {
    fn empty() -> Self {
        let e = (f32::MAX, f32::MIN);
        Self {
            before: [e; 2],
            after: [e; 2],
        }
    }
}

fn load_f(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store_f(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// Live readings for the UI, all dB.
pub struct Meters {
    in_l: AtomicU32,
    in_r: AtomicU32,
    out_l: AtomicU32,
    out_r: AtomicU32,
    wet: AtomicU32,
    dry: AtomicU32,
    avg_before: AtomicU32,
    avg_after: AtomicU32,
    auto_makeup: AtomicU32,
    loop_level: AtomicU32,
    duck: AtomicU32,
    delay_ms: AtomicU32,
    bpm: AtomicU32,
    host_tempo: AtomicBool,
}

impl Default for Meters {
    fn default() -> Self {
        let quiet = || AtomicU32::new((-120f32).to_bits());
        Self {
            in_l: quiet(),
            in_r: quiet(),
            out_l: quiet(),
            out_r: quiet(),
            wet: quiet(),
            dry: quiet(),
            avg_before: quiet(),
            avg_after: quiet(),
            auto_makeup: AtomicU32::new(0),
            loop_level: quiet(),
            duck: AtomicU32::new(0),
            delay_ms: AtomicU32::new(375f32.to_bits()),
            bpm: AtomicU32::new(120f32.to_bits()),
            host_tempo: AtomicBool::new(false),
        }
    }
}

impl Meters {
    /// Input peak per channel (falls at 20 dB/s), dBFS.
    pub fn input_lr(&self) -> (f32, f32) {
        (load_f(&self.in_l), load_f(&self.in_r))
    }
    /// Output peak per channel, dBFS.
    pub fn output_lr(&self) -> (f32, f32) {
        (load_f(&self.out_l), load_f(&self.out_r))
    }
    /// Output peak, dBFS.
    pub fn output(&self) -> f32 {
        let (l, r) = self.output_lr();
        l.max(r)
    }
    /// Wet part of the output, dBFS.
    pub fn wet(&self) -> f32 {
        load_f(&self.wet)
    }
    /// Dry part of the output, dBFS.
    pub fn dry(&self) -> f32 {
        load_f(&self.dry)
    }
    /// Post meter: average peak of the input (BEFORE), dBFS.
    pub fn avg_before(&self) -> f32 {
        load_f(&self.avg_before)
    }
    /// Post meter: average peak of the output (AFTER), dBFS.
    pub fn avg_after(&self) -> f32 {
        load_f(&self.avg_after)
    }
    /// Gain Out that auto-makeup is applying now, dB.
    pub fn auto_makeup(&self) -> f32 {
        load_f(&self.auto_makeup)
    }
    /// Level circulating in the feedback loop (falls at 20 dB/s), dBFS: the feedback meter.
    pub fn loop_level(&self) -> f32 {
        load_f(&self.loop_level)
    }
    /// Ducking gain reduction, dB.
    pub fn duck(&self) -> f32 {
        load_f(&self.duck)
    }
    /// The delay time in use, ms.
    pub fn delay_ms(&self) -> f32 {
        load_f(&self.delay_ms)
    }
    /// The tempo in use, BPM (the host's when it sends one).
    pub fn bpm(&self) -> f32 {
        load_f(&self.bpm)
    }
    /// Whether the tempo comes from the host.
    pub fn host_tempo(&self) -> bool {
        self.host_tempo.load(Ordering::Relaxed)
    }
}

/// A/B: two complete settings snapshots (everything but Bypass).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AbState {
    /// 0 = A, 1 = B.
    pub active: usize,
    /// Stored snapshots (id, value); the active slot is refreshed when switching away.
    pub slots: [Option<Vec<(u32, f64)>>; 2],
}

struct Shared {
    params: crate::params::Values,
    meters: Meters,
    latency: AtomicU32,
    sample_rate: AtomicU32,
    latency_changed: AtomicBool,
    reset_post_meter: AtomicBool,
    stacked: AtomicBool,
    listener: Mutex<Option<Arc<dyn Fn(HostEvent) + Send + Sync>>>,
    columns: Mutex<Option<rtrb::Consumer<Column>>>,
    ab: Mutex<AbState>,
    preset: Mutex<Option<usize>>,
    tap: Mutex<TapTempo>,
    epoch: std::time::Instant,
}

/// The engine, shared by the plugin glue, the editor and Solara.
pub struct Engine {
    shared: Arc<Shared>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

const MAGIC: &[u8; 8] = b"SPDL01\0\0";

#[derive(Serialize, Deserialize)]
struct State {
    params: Vec<(u32, f64)>,
    #[serde(default)]
    ab: AbState,
    #[serde(default)]
    preset: Option<usize>,
    #[serde(default)]
    stacked: bool,
}

impl Engine {
    /// A new engine with default settings.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                params: crate::params::Values::default(),
                meters: Meters::default(),
                latency: AtomicU32::new(u32::MAX),
                sample_rate: AtomicU32::new(48_000f32.to_bits()),
                latency_changed: AtomicBool::new(false),
                reset_post_meter: AtomicBool::new(false),
                stacked: AtomicBool::new(false),
                listener: Mutex::new(None),
                columns: Mutex::new(None),
                ab: Mutex::new(AbState::default()),
                preset: Mutex::new(None),
                tap: Mutex::new(TapTempo::default()),
                epoch: std::time::Instant::now(),
            }),
        }
    }

    /// Parameter values.
    pub fn params(&self) -> &crate::params::Values {
        &self.shared.params
    }

    /// Meters.
    pub fn meters(&self) -> &Meters {
        &self.shared.meters
    }

    /// Sets a parameter from the host (no echo back to the host).
    pub fn set_from_host(&self, id: Pid, plain: f64) {
        self.shared.params.set(id, plain);
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

    /// UI: start of a gesture.
    pub fn begin_edit(&self, id: Pid) {
        self.notify(HostEvent::Begin(id));
    }

    /// UI: new value inside a gesture.
    pub fn edit(&self, id: Pid, plain: f64) {
        let v = self.shared.params.set(id, plain);
        self.notify(HostEvent::Value(id, v));
    }

    /// UI: end of a gesture.
    pub fn end_edit(&self, id: Pid) {
        self.notify(HostEvent::End(id));
    }

    /// UI: a whole click (begin, value, end). Skips values that didn't change.
    pub fn set(&self, id: Pid, plain: f64) {
        if let Some(info) = id.info()
            && (self.shared.params.get(id) - info.sanitize(plain)).abs() < 1e-9
        {
            return;
        }
        self.begin_edit(id);
        self.edit(id, plain);
        self.end_edit(id);
    }

    /// Active version.
    pub fn version(&self) -> Version {
        self.shared.params.version()
    }

    /// Switches version. Each version keeps its own settings.
    pub fn set_version(&self, v: Version) {
        self.set(Pid::g(G::Version), v.index() as f64);
    }

    /// Stacked view (hardware on top) instead of digital-first.
    pub fn stacked(&self) -> bool {
        self.shared.stacked.load(Ordering::Relaxed)
    }

    /// Switches the view mode (saved with the session).
    pub fn set_stacked(&self, on: bool) {
        self.shared.stacked.store(on, Ordering::Relaxed);
    }

    /// Latency for the host, samples: the active version's.
    pub fn latency(&self) -> u32 {
        let p = &self.shared.params;
        latency_for(p.version(), &p.settings(), self.sample_rate()) as u32
    }

    /// Latency of a version with its current settings, ms (for the UI).
    pub fn latency_ms(&self, v: Version) -> f32 {
        let sr = self.sample_rate();
        latency_for(v, &self.shared.params.settings(), sr) as f32 * 1000.0 / sr
    }

    /// True once after the latency changed (the glue tells the host).
    pub fn take_latency_changed(&self) -> bool {
        self.shared.latency_changed.swap(false, Ordering::AcqRel)
    }

    /// The host's sample rate, before activation.
    pub fn set_sample_rate(&self, sr: f32) {
        if sr.is_finite() && sr > 1000.0 {
            store_f(&self.shared.sample_rate, sr);
        }
    }

    /// Sample rate.
    pub fn sample_rate(&self) -> f32 {
        load_f(&self.shared.sample_rate)
    }

    /// Drains the display columns (UI thread).
    pub fn drain_columns(&self, mut f: impl FnMut(Column)) {
        if let Ok(mut c) = self.shared.columns.lock()
            && let Some(rx) = c.as_mut()
        {
            while let Ok(col) = rx.pop() {
                f(col);
            }
        }
    }

    /// MATCH: sets the active version's Gain Out so the output's average peak matches the
    /// input's. Turns auto-makeup off (MATCH is the press-once version).
    pub fn match_now(&self) {
        let m = &self.shared.meters;
        let (before, after) = (m.avg_before(), m.avg_after());
        if before < -100.0 || after < -100.0 {
            return;
        }
        let v = self.version();
        let p = &self.shared.params;
        let current = if p.c(v, Ctl::AutoMakeup) >= 0.5 {
            m.auto_makeup()
        } else {
            p.c(v, Ctl::GainOut) as f32
        };
        self.set(Pid::c(v, Ctl::AutoMakeup), 0.0);
        self.set(
            Pid::c(v, Ctl::GainOut),
            f64::from((current + before - after).clamp(-24.0, 24.0)),
        );
        self.shared.reset_post_meter.store(true, Ordering::Relaxed);
    }

    /// Tap tempo, now. See [`Engine::tap_at`].
    pub fn tap(&self) {
        let t = self.shared.epoch.elapsed().as_secs_f64();
        self.tap_at(t);
    }

    /// Tap tempo at `t` seconds: from the second tap on, the active version's Time follows
    /// the gaps between taps and tempo-sync turns off (the taps set the time by hand).
    /// Returns the new time, ms.
    pub fn tap_at(&self, t: f64) -> Option<f32> {
        let ms = self.shared.tap.lock().ok()?.tap(t)?;
        let v = self.version();
        self.set(Pid::c(v, Ctl::Sync), 0.0);
        self.set(Pid::c(v, Ctl::Time), f64::from(ms));
        Some(ms)
    }

    /// Turns auto-makeup on or off for the active version. Turning it off keeps the gain it
    /// had reached, written into Gain Out.
    pub fn set_auto_makeup(&self, on: bool) {
        let v = self.version();
        if !on && self.shared.params.c(v, Ctl::AutoMakeup) >= 0.5 {
            let g = self.shared.meters.auto_makeup();
            self.set(Pid::c(v, Ctl::GainOut), f64::from(g));
        }
        self.set(Pid::c(v, Ctl::AutoMakeup), if on { 1.0 } else { 0.0 });
    }

    /// Resets a version's controls.
    pub fn reset_version(&self, v: Version) {
        for info in TABLE.iter() {
            if matches!(info.id.decode(), Some(Which::C(x, _) | Which::X(x, _)) if x == v) {
                self.set(info.id, info.default);
            }
        }
    }

    /// Loads factory preset `i`.
    pub fn load_preset(&self, i: usize) {
        let Some(p) = PRESETS.get(i) else {
            return;
        };
        self.reset_version(p.version);
        for (c, v) in p.ctl {
            self.set(Pid::c(p.version, *c), *v);
        }
        for (k, v) in p.extras {
            self.set(Pid::x(p.version, *k), *v);
        }
        self.set_version(p.version);
        if let Ok(mut s) = self.shared.preset.lock() {
            *s = Some(i);
        }
    }

    /// The preset last loaded, if any.
    pub fn preset(&self) -> Option<usize> {
        self.shared.preset.lock().ok().and_then(|p| *p)
    }

    fn snapshot(&self) -> Vec<(u32, f64)> {
        TABLE
            .iter()
            .filter(|i| i.id != Pid::g(G::Bypass))
            .map(|i| (i.id.0, self.shared.params.get(i.id)))
            .collect()
    }

    fn apply(&self, snap: &[(u32, f64)]) {
        // Version last, so the knobs are in place when the version switches.
        for (id, v) in snap.iter().filter(|(id, _)| *id != G::Version as u32) {
            self.set(Pid(*id), *v);
        }
        if let Some((_, v)) = snap.iter().find(|(id, _)| *id == G::Version as u32) {
            self.set(Pid::g(G::Version), *v);
        }
    }

    /// Which A/B slot is live (0 = A).
    pub fn ab(&self) -> usize {
        self.shared.ab.lock().map_or(0, |a| a.active)
    }

    /// Switches to A (0) or B (1). The first switch to B starts it as a copy of A.
    pub fn select_ab(&self, slot: usize) {
        let slot = slot.min(1);
        let current = self.snapshot();
        let target = {
            let Ok(mut ab) = self.shared.ab.lock() else {
                return;
            };
            if ab.active == slot {
                return;
            }
            let from = ab.active;
            ab.slots[from] = Some(current.clone());
            ab.active = slot;
            ab.slots[slot].get_or_insert(current).clone()
        };
        self.apply(&target);
    }

    /// Copies the live settings into the other slot.
    pub fn copy_ab(&self) {
        let current = self.snapshot();
        if let Ok(mut ab) = self.shared.ab.lock() {
            let other = 1 - ab.active;
            ab.slots[other] = Some(current);
        }
    }

    /// A processor for the audio thread (one per activation).
    pub fn processor(self: &Arc<Self>, sample_rate: f64, _max_block: usize) -> Processor {
        let sr = sample_rate as f32;
        self.set_sample_rate(sr);
        let (tx, rx) = rtrb::RingBuffer::new(4096);
        if let Ok(mut c) = self.shared.columns.lock() {
            *c = Some(rx);
        }
        self.shared.latency.store(self.latency(), Ordering::Relaxed);
        Processor {
            shared: self.shared.clone(),
            fx: Box::new(DelayFx::new(sr)),
            col_len: column_len(sr),
            col_n: 0,
            col: Column::empty(),
            tx,
            scratch: [[0.0; CHUNK]; 2],
            fall: 20.0 / sr,
            levels: [-120.0; 7],
        }
    }

    /// Serialises every parameter, the A/B slots and the view mode.
    pub fn save_state(&self) -> Vec<u8> {
        let state = State {
            params: TABLE
                .iter()
                .map(|i| (i.id.0, self.shared.params.get(i.id)))
                .collect(),
            ab: self.shared.ab.lock().map(|a| a.clone()).unwrap_or_default(),
            preset: self.preset(),
            stacked: self.stacked(),
        };
        let mut out = MAGIC.to_vec();
        out.extend(serde_json::to_vec(&state).unwrap_or_default());
        out
    }

    /// Restores what [`Engine::save_state`] wrote. Returns false, changing nothing, for
    /// data that isn't a SolPlugs Delay state.
    pub fn load_state(&self, data: &[u8]) -> bool {
        let Some(json) = data.strip_prefix(MAGIC.as_slice()) else {
            return false;
        };
        let Ok(state) = serde_json::from_slice::<State>(json) else {
            return false;
        };
        for (id, v) in &state.params {
            self.shared.params.set(Pid(*id), *v);
        }
        if let Ok(mut ab) = self.shared.ab.lock() {
            *ab = state.ab;
            ab.active = ab.active.min(1);
        }
        if let Ok(mut p) = self.shared.preset.lock() {
            *p = state.preset.filter(|i| *i < PRESETS.len());
        }
        self.set_stacked(state.stacked);
        true
    }
}

/// Samples per display column: about 5 ms.
fn column_len(sr: f32) -> usize {
    (sr / 200.0).round().max(16.0) as usize
}

/// The audio-thread half: no locks, no allocation.
pub struct Processor {
    shared: Arc<Shared>,
    fx: Box<DelayFx>,
    col_len: usize,
    col_n: usize,
    col: Column,
    tx: rtrb::Producer<Column>,
    scratch: [[f32; CHUNK]; 2],
    /// Meter fall, dB per sample.
    fall: f32,
    /// Displayed in L/R, out L/R, wet, dry and loop peaks (dB) with fall-back.
    levels: [f32; 7],
}

impl Processor {
    /// Latency, samples, of the active version as last processed.
    pub fn latency(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }

    /// Processes `io` in place (1 or 2 channels). `sc` is the external sidechain (inputs
    /// 3/4) when the host gives one, as long as `io`; `host_bpm` the song tempo when the
    /// host sends one (else the Tempo parameter is used).
    pub fn process(
        &mut self,
        io: &mut [&mut [f32]],
        sc: Option<[&[f32]; 2]>,
        host_bpm: Option<f64>,
    ) {
        let p = &self.shared.params;
        let mut g = p.global();
        let host = host_bpm.filter(|b| b.is_finite() && *b >= 1.0);
        if let Some(b) = host {
            g.bpm = b as f32;
        }
        let m = &self.shared.meters;
        m.host_tempo.store(host.is_some(), Ordering::Relaxed);
        store_f(&m.bpm, g.bpm);
        let s = p.settings();
        if self.shared.reset_post_meter.swap(false, Ordering::Relaxed) {
            self.fx.reset_meters();
        }
        let lat = latency_for(g.version, &s, self.fx.sample_rate()) as u32;
        if self.shared.latency.swap(lat, Ordering::Relaxed) != lat {
            self.shared.latency_changed.store(true, Ordering::Release);
        }
        let channels = io.len().min(2);
        if channels == 0 {
            return;
        }
        let n = io.iter().take(channels).map(|c| c.len()).min().unwrap_or(0);
        let mut start = 0;
        while start < n {
            let left_in_col = self.col_len - self.col_n;
            let len = (n - start).min(CHUNK).min(left_in_col);
            let range = start..start + len;
            for c in 0..channels {
                self.scratch[c][..len].copy_from_slice(&io[c][range.clone()]);
            }
            let key = sc.and_then(|k| {
                (k[0].len() >= start + len && k[1].len() >= start + len)
                    .then(|| [&k[0][range.clone()], &k[1][range.clone()]])
            });
            let (a, b) = io.split_at_mut(1);
            let stats = if channels > 1 {
                let mut piece = [&mut a[0][range.clone()], &mut b[0][range.clone()]];
                self.fx.process(&mut piece, key, &g, &s)
            } else {
                let mut piece = [&mut a[0][range.clone()]];
                self.fx.process(&mut piece, key, &g, &s)
            };
            self.accumulate(io, channels, start, len, &stats);
            start += len;
        }
    }

    fn accumulate(
        &mut self,
        io: &[&mut [f32]],
        channels: usize,
        start: usize,
        len: usize,
        stats: &solplugs_delay_core::Stats,
    ) {
        for i in 0..len {
            for c in 0..2 {
                let src = c.min(channels - 1);
                let x = self.scratch[src][i];
                let y = io[src][start + i];
                let b = &mut self.col.before[c];
                *b = (b.0.min(x), b.1.max(x));
                let a = &mut self.col.after[c];
                *a = (a.0.min(y), a.1.max(y));
            }
        }
        self.col_n += len;
        if self.col_n >= self.col_len {
            let _ = self.tx.push(self.col);
            self.col = Column::empty();
            self.col_n = 0;
        }
        let fall = self.fall * len as f32;
        let m = &self.shared.meters;
        for (slot, (v, a)) in self.levels.iter_mut().zip([
            (stats.in_lr[0], &m.in_l),
            (stats.in_lr[1], &m.in_r),
            (stats.out_lr[0], &m.out_l),
            (stats.out_lr[1], &m.out_r),
            (stats.wet_peak, &m.wet),
            (stats.dry_peak, &m.dry),
            (stats.loop_peak, &m.loop_level),
        ]) {
            let db = lin_to_db(v);
            *slot = if db > *slot {
                db
            } else {
                (*slot - fall).max(db)
            };
            store_f(a, *slot);
        }
        store_f(&m.avg_before, stats.avg_before_db);
        store_f(&m.avg_after, stats.avg_after_db);
        store_f(&m.auto_makeup, stats.auto_makeup_db);
        store_f(&m.duck, stats.duck_db);
        store_f(&m.delay_ms, stats.delay_ms);
    }
}

/// The parameter table, re-exported for the glue.
pub use crate::params::TABLE as PARAMS;
