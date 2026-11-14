//! The engine: parameters, meters, the audio-thread [`Processor`], presets, A/B, MATCH and
//! auto-makeup.
//!
//! Threads:
//! - audio thread: [`Processor::process`] (no locks, no allocation);
//! - UI thread: reads meters, drains the display columns, edits parameters.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use solplugs_comp_core::util::lin_to_db;
use solplugs_comp_core::{CHUNK, Compressor, Mode};

use crate::params::{self, G, K, Pid, TABLE, Values};
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

/// One display column (about 5 ms of audio): waveform extents and gain reduction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Column {
    /// Input (BEFORE) min and max.
    pub before: (f32, f32),
    /// Output (AFTER) min and max.
    pub after: (f32, f32),
    /// Gain reduction, dB (positive), largest of the channels.
    pub gr: f32,
}

fn load_f(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store_f(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// Live readings for the UI, all dB.
pub struct Meters {
    input: AtomicU32,
    output: AtomicU32,
    gr_l: AtomicU32,
    gr_r: AtomicU32,
    wet: AtomicU32,
    dry: AtomicU32,
    avg_before: AtomicU32,
    avg_after: AtomicU32,
    auto_makeup: AtomicU32,
}

impl Default for Meters {
    fn default() -> Self {
        let quiet = || AtomicU32::new((-120f32).to_bits());
        Self {
            input: quiet(),
            output: quiet(),
            gr_l: AtomicU32::new(0),
            gr_r: AtomicU32::new(0),
            wet: quiet(),
            dry: quiet(),
            avg_before: quiet(),
            avg_after: quiet(),
            auto_makeup: AtomicU32::new(0),
        }
    }
}

impl Meters {
    /// Input peak level (falls at 20 dB/s), dBFS.
    pub fn input(&self) -> f32 {
        load_f(&self.input)
    }
    /// Output peak level, dBFS.
    pub fn output(&self) -> f32 {
        load_f(&self.output)
    }
    /// Gain reduction, left and right, dB (positive).
    pub fn gr(&self) -> (f32, f32) {
        (load_f(&self.gr_l), load_f(&self.gr_r))
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
    /// Gain out that auto-makeup is applying now, dB.
    pub fn auto_makeup(&self) -> f32 {
        load_f(&self.auto_makeup)
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
    params: Values,
    meters: Meters,
    latency: AtomicU32,
    sample_rate: AtomicU32,
    latency_changed: AtomicBool,
    reset_post_meter: AtomicBool,
    listener: Mutex<Option<Arc<dyn Fn(HostEvent) + Send + Sync>>>,
    columns: Mutex<Option<rtrb::Consumer<Column>>>,
    ab: Mutex<AbState>,
    preset: Mutex<Option<usize>>,
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

const MAGIC: &[u8; 8] = b"SPCOMP1\0";

#[derive(Serialize, Deserialize)]
struct State {
    params: Vec<(u32, f64)>,
    #[serde(default)]
    ab: AbState,
    #[serde(default)]
    preset: Option<usize>,
}

impl Engine {
    /// A new engine with default settings.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                params: Values::default(),
                meters: Meters::default(),
                latency: AtomicU32::new(u32::MAX),
                sample_rate: AtomicU32::new(48_000f32.to_bits()),
                latency_changed: AtomicBool::new(false),
                reset_post_meter: AtomicBool::new(false),
                listener: Mutex::new(None),
                columns: Mutex::new(None),
                ab: Mutex::new(AbState::default()),
                preset: Mutex::new(None),
            }),
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

    /// Active mode.
    pub fn mode(&self) -> Mode {
        self.shared.params.mode()
    }

    /// Switches mode. Each mode keeps its own settings.
    pub fn set_mode(&self, mode: Mode) {
        self.set(Pid::g(G::Mode), mode.index() as f64);
    }

    /// Latency for the host, samples: the active mode's.
    pub fn latency(&self) -> u32 {
        let p = &self.shared.params;
        let mode = p.mode();
        let s = p.settings(mode);
        solplugs_comp_core::Chain::latency_for(mode, &s, self.sample_rate()) as u32
    }

    /// Latency of a mode with its current settings, ms (for the UI).
    pub fn latency_ms(&self, mode: Mode) -> f32 {
        let s = self.shared.params.settings(mode);
        let sr = self.sample_rate();
        solplugs_comp_core::Chain::latency_for(mode, &s, sr) as f32 * 1000.0 / sr
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

    /// Display columns per second.
    pub fn columns_per_second(&self) -> f32 {
        self.sample_rate() / column_len(self.sample_rate()) as f32
    }

    /// MATCH: sets the active mode's gain out so the output's average peak matches the
    /// input's. Turns auto-makeup off (MATCH is the press-once version).
    pub fn match_now(&self) {
        let m = &self.shared.meters;
        let (before, after) = (m.avg_before(), m.avg_after());
        if before < -100.0 || after < -100.0 {
            return;
        }
        let mode = self.mode();
        let p = &self.shared.params;
        let current = if p.m(mode, K::AutoMakeup) >= 0.5 {
            m.auto_makeup()
        } else {
            p.m(mode, K::GainOut) as f32
        };
        self.set(Pid::m(mode, K::AutoMakeup), 0.0);
        self.set(
            Pid::m(mode, K::GainOut),
            f64::from((current + before - after).clamp(-24.0, 24.0)),
        );
        self.shared.reset_post_meter.store(true, Ordering::Relaxed);
    }

    /// Turns auto-makeup on or off for the active mode. Turning it off keeps the gain it
    /// had reached, written into Gain Out.
    pub fn set_auto_makeup(&self, on: bool) {
        let mode = self.mode();
        if !on && self.shared.params.m(mode, K::AutoMakeup) >= 0.5 {
            let g = self.shared.meters.auto_makeup();
            self.set(Pid::m(mode, K::GainOut), f64::from(g));
        }
        self.set(Pid::m(mode, K::AutoMakeup), if on { 1.0 } else { 0.0 });
    }

    /// Loads factory preset `i`.
    pub fn load_preset(&self, i: usize) {
        let Some(p) = PRESETS.get(i) else {
            return;
        };
        for k in K::ALL {
            if !k.in_mode(p.mode) {
                continue;
            }
            let id = Pid::m(p.mode, k);
            let v = p
                .values
                .iter()
                .find(|(pk, _)| *pk == k)
                .map(|(_, v)| *v)
                .or_else(|| id.info().map(|i| i.default))
                .unwrap_or(0.0);
            self.set(id, v);
        }
        self.set_mode(p.mode);
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
        // Mode last, so the knobs are in place when the mode switches.
        for (id, v) in snap.iter().filter(|(id, _)| *id != G::Mode as u32) {
            self.set(Pid(*id), *v);
        }
        if let Some((_, v)) = snap.iter().find(|(id, _)| *id == G::Mode as u32) {
            self.set(Pid::g(G::Mode), *v);
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
            comp: Compressor::new(sr),
            col_len: column_len(sr),
            col: ColAcc::default(),
            tx,
            scratch: [[0.0; CHUNK]; 2],
            fall: 20.0 / sr,
            levels: [-120.0; 4],
        }
    }

    /// Serialises every parameter and the A/B slots.
    pub fn save_state(&self) -> Vec<u8> {
        let state = State {
            params: TABLE
                .iter()
                .map(|i| (i.id.0, self.shared.params.get(i.id)))
                .collect(),
            ab: self.shared.ab.lock().map(|a| a.clone()).unwrap_or_default(),
            preset: self.preset(),
        };
        let mut out = MAGIC.to_vec();
        out.extend(serde_json::to_vec(&state).unwrap_or_default());
        out
    }

    /// Restores what [`Engine::save_state`] wrote. Returns false, changing nothing, for
    /// data that isn't a SolPlugs Compressor state.
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
        true
    }
}

/// Samples per display column: about 5 ms.
fn column_len(sr: f32) -> usize {
    (sr / 200.0).round().max(16.0) as usize
}

#[derive(Clone, Copy, Debug)]
struct ColAcc {
    n: usize,
    col: Column,
}

impl Default for ColAcc {
    fn default() -> Self {
        Self {
            n: 0,
            col: Column {
                before: (f32::MAX, f32::MIN),
                after: (f32::MAX, f32::MIN),
                gr: 0.0,
            },
        }
    }
}

/// The audio-thread half: no locks, no allocation.
pub struct Processor {
    shared: Arc<Shared>,
    comp: Compressor,
    col_len: usize,
    col: ColAcc,
    tx: rtrb::Producer<Column>,
    scratch: [[f32; CHUNK]; 2],
    fall: f32,
    /// Meter fall, dB per sample.
    /// Displayed input, output, wet, dry peaks (dB) with fall-back.
    levels: [f32; 4],
}

impl Processor {
    /// Latency, samples, of the active mode as last processed.
    pub fn latency(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }

    /// Processes `io` in place (1 or 2 channels). `sc` is the external sidechain input
    /// (channels 3/4), if the host gives one.
    pub fn process(&mut self, io: &mut [&mut [f32]], sc: Option<&[&[f32]]>) {
        let p = &self.shared.params;
        let g = p.global();
        let all = p.all_settings();
        if self.shared.reset_post_meter.swap(false, Ordering::Relaxed) {
            self.comp.reset_meters();
        }
        let lat = solplugs_comp_core::Chain::latency_for(
            g.mode,
            &all[g.mode.index()],
            self.comp.sample_rate(),
        ) as u32;
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
            // Pieces end on column boundaries so each column gets its own GR reading.
            let left_in_col = self.col_len - self.col.n;
            let len = (n - start).min(CHUNK).min(left_in_col);
            let range = start..start + len;
            for c in 0..channels {
                self.scratch[c][..len].copy_from_slice(&io[c][range.clone()]);
            }
            let sc_arr = sc.and_then(|s| {
                let a = s.first()?.get(range.clone())?;
                let b = s.get(1).and_then(|c| c.get(range.clone())).unwrap_or(a);
                Some([a, b])
            });
            let sc_ref: Option<&[&[f32]]> = sc_arr.as_ref().map(|a| a.as_slice());
            let (a, b) = io.split_at_mut(1);
            let stats = if channels > 1 {
                let mut piece = [&mut a[0][range.clone()], &mut b[0][range.clone()]];
                self.comp.process(&mut piece, sc_ref, &g, &all)
            } else {
                let mut piece = [&mut a[0][range.clone()]];
                self.comp.process(&mut piece, sc_ref, &g, &all)
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
        stats: &solplugs_comp_core::Stats,
    ) {
        let gr = stats.gr[0].max(stats.gr[1]);
        for i in 0..len {
            let mut bmin = f32::MAX;
            let mut bmax = f32::MIN;
            let mut amin = f32::MAX;
            let mut amax = f32::MIN;
            for c in 0..channels {
                let x = self.scratch[c][i];
                let y = io[c][start + i];
                bmin = bmin.min(x);
                bmax = bmax.max(x);
                amin = amin.min(y);
                amax = amax.max(y);
            }
            let col = &mut self.col.col;
            col.before = (col.before.0.min(bmin), col.before.1.max(bmax));
            col.after = (col.after.0.min(amin), col.after.1.max(amax));
        }
        self.col.col.gr = self.col.col.gr.max(gr);
        self.col.n += len;
        if self.col.n >= self.col_len {
            let _ = self.tx.push(self.col.col);
            self.col = ColAcc::default();
        }
        // Meters: peaks that fall back at 20 dB/s.
        let fall = self.fall * len as f32;
        let m = &self.shared.meters;
        for (slot, (v, a)) in self.levels.iter_mut().zip([
            (stats.in_peak, &m.input),
            (stats.out_peak, &m.output),
            (stats.wet_peak, &m.wet),
            (stats.dry_peak, &m.dry),
        ]) {
            let db = lin_to_db(v);
            *slot = if db > *slot {
                db
            } else {
                (*slot - fall).max(db)
            };
            store_f(a, *slot);
        }
        store_f(&m.gr_l, stats.gr[0]);
        store_f(
            &m.gr_r,
            if channels > 1 {
                stats.gr[1]
            } else {
                stats.gr[0]
            },
        );
        store_f(&m.avg_before, stats.avg_before_db);
        store_f(&m.avg_after, stats.avg_after_db);
        store_f(&m.auto_makeup, stats.auto_makeup_db);
    }
}

/// The parameter table, re-exported for the glue.
pub use params::TABLE as PARAMS;
