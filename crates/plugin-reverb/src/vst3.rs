//! The VST3 plugin: one "single component" class (component, processor and edit controller
//! in one object, which every major host supports), its editor view, and the factory.
//!
//! This is the crate's FFI boundary. Every `unsafe` block dereferences a pointer the host
//! passed us under the VST3 contract; each says which.
#![allow(unsafe_code)]
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use solplugs_reverb_app::engine::{Engine, HostEvent, Processor};
use solplugs_reverb_app::params::{self, G, Kind, Pid, TABLE, Which};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, uid};

use crate::editor::{DEFAULT_SIZE, MIN_SIZE};

/// Class id of the single component.
const CID: TUID = uid(0x540B_FD82, 0xC792_4A64, 0x813E_7CFC, 0x7A4D_F608);
/// Largest state we accept from the host.
const MAX_STATE: usize = 1 << 24;

/// The host's component handler, moved into the engine's UI-edit listener.
///
/// SAFETY of the Send/Sync impls: the listener only runs from the editor's UI callbacks,
/// and the editor is a child window of the host's own window, so its messages are handled
/// on the host's UI thread, the thread VST3 requires for `IComponentHandler` calls.
struct HostHandler(ComPtr<IComponentHandler>);
unsafe impl Send for HostHandler {}
unsafe impl Sync for HostHandler {}

#[allow(clippy::unnecessary_cast)] // TChar is u16 on some targets only
fn str_from_tchar(src: &[TChar]) -> String {
    let units: Vec<u16> = src
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u16)
        .collect();
    String::from_utf16_lossy(&units)
}

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    for (d, s) in dst.iter_mut().zip(src.bytes().take(n)) {
        *d = s as c_char;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut len = 0;
    for (d, s) in dst.iter_mut().zip(src.encode_utf16()) {
        *d = s as TChar;
        len += 1;
    }
    let end = len.min(dst.len().saturating_sub(1));
    if let Some(e) = dst.get_mut(end) {
        *e = 0;
    }
}

/// Channels in a speaker arrangement (mono or stereo here).
fn channel_count(arr: SpeakerArrangement) -> int32 {
    if arr == SpeakerArr::kMono { 1 } else { 2 }
}

/// The plugin object.
struct Verb {
    engine: Arc<Engine>,
    processor: Mutex<Option<Processor>>,
    sample_rate: AtomicU64,
    max_block: AtomicU32,
    handler: Arc<Mutex<Option<HostHandler>>>,
    /// Speaker arrangement of the main bus (mono or stereo, in = out).
    main_arr: AtomicU64,
}

impl Class for Verb {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IProcessContextRequirements,
        IEditController,
    );
}

impl Verb {
    fn new() -> Self {
        let engine = Arc::new(Engine::new());
        let handler: Arc<Mutex<Option<HostHandler>>> = Arc::new(Mutex::new(None));
        let h = handler.clone();
        engine.set_listener(Some(Arc::new(move |e| {
            let Ok(guard) = h.lock() else {
                return;
            };
            let Some(HostHandler(handler)) = guard.as_ref() else {
                return;
            };
            // SAFETY: see HostHandler: called on the host's UI thread with a live handler.
            unsafe {
                match e {
                    HostEvent::Begin(p) => {
                        handler.beginEdit(p.0);
                    }
                    HostEvent::Value(p, v) => {
                        if let Some(i) = p.info() {
                            handler.performEdit(p.0, i.normalize(v));
                        }
                    }
                    HostEvent::End(p) => {
                        handler.endEdit(p.0);
                    }
                }
            }
        })));
        Self {
            engine,
            processor: Mutex::new(None),
            sample_rate: AtomicU64::new(48_000f64.to_bits()),
            max_block: AtomicU32::new(4096),
            handler,
            main_arr: AtomicU64::new(SpeakerArr::kStereo),
        }
    }
}

impl IPluginBaseTrait for Verb {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        if let Ok(mut p) = self.processor.lock() {
            *p = None;
        }
        if let Ok(mut h) = self.handler.lock() {
            *h = None;
        }
        self.engine.set_listener(None);
        kResultOk
    }
}

impl IComponentTrait for Verb {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        // Single component: the host queries IEditController from this object.
        kNotImplemented
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> int32 {
        if media_type != MediaTypes_::kAudio as MediaType {
            0
        } else {
            let _ = dir;
            1
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        let input = dir == BusDirections_::kInput as BusDirection;
        if media_type != MediaTypes_::kAudio as MediaType
            || index != 0
            || bus.is_null()
        {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid, writable BusInfo (checked non-null above).
        let bus = unsafe { &mut *bus };
        bus.mediaType = MediaTypes_::kAudio as MediaType;
        bus.direction = dir;
        bus.channelCount = channel_count(self.main_arr.load(Ordering::Relaxed));
        copy_wstring(if input { "Main In" } else { "Main Out" }, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as uint32;
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media_type: MediaType,
        _dir: BusDirection,
        _index: int32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        let Ok(mut p) = self.processor.lock() else {
            return kResultFalse;
        };
        if state != 0 {
            let sr = f64::from_bits(self.sample_rate.load(Ordering::Relaxed));
            let max = self.max_block.load(Ordering::Relaxed) as usize;
            *p = Some(self.engine.processor(sr, max));
        } else {
            *p = None;
        }
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: `state` is a live IBStream for the duration of this call (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut read: int32 = 0;
            // SAFETY: `buf` is writable for its full length; `read` is a valid out pointer.
            let r = unsafe {
                stream.read(
                    buf.as_mut_ptr().cast::<c_void>(),
                    buf.len() as int32,
                    &mut read,
                )
            };
            if r != kResultOk || read <= 0 {
                break;
            }
            data.extend_from_slice(&buf[..(read as usize).min(buf.len())]);
            if data.len() > MAX_STATE {
                return kResultFalse;
            }
        }
        if self.engine.load_state(&data) {
            if let Ok(h) = self.handler.lock()
                && let Some(HostHandler(h)) = h.as_ref()
            {
                // SAFETY: see HostHandler: setState runs on the host's UI thread.
                unsafe {
                    h.restartComponent(RestartFlags_::kParamValuesChanged);
                }
            }
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: `state` is a live IBStream for the duration of this call (or null).
        let Some(stream) = (unsafe { ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut data = self.engine.save_state();
        let mut written: int32 = 0;
        // SAFETY: `data` is valid for its length; the host only reads from it.
        let r = unsafe {
            stream.write(
                data.as_mut_ptr().cast::<c_void>(),
                data.len() as int32,
                &mut written,
            )
        };
        if r == kResultOk {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

impl IAudioProcessorTrait for Verb {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: int32,
        outputs: *mut SpeakerArrangement,
        num_outs: int32,
    ) -> tresult {
        if num_ins != 1 || num_outs != 1 || inputs.is_null() || outputs.is_null() {
            return kResultFalse;
        }
        // SAFETY: the host passes arrays of `num_ins` / `num_outs` arrangements (checked).
        let (main_in, out) = unsafe { (*inputs, *outputs) };
        let ok = |a: SpeakerArrangement| a == SpeakerArr::kStereo || a == SpeakerArr::kMono;
        if main_in != out || !ok(out) {
            return kResultFalse;
        }
        self.main_arr.store(out, Ordering::Relaxed);
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: int32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        let _ = dir;
        if arr.is_null() || index != 0 {
            return kInvalidArgument;
        }
        // SAFETY: non-null out pointer from the host.
        unsafe { *arr = self.main_arr.load(Ordering::Relaxed) };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, symbolic_sample_size: int32) -> tresult {
        if symbolic_sample_size == SymbolicSampleSizes_::kSample32 as int32 {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn getLatencySamples(&self) -> uint32 {
        self.engine.latency()
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null ProcessSetup from the host, read only.
        let setup = unsafe { &*setup };
        if setup.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as int32 {
            return kResultFalse;
        }
        self.sample_rate
            .store(setup.sampleRate.to_bits(), Ordering::Relaxed);
        self.max_block
            .store(setup.maxSamplesPerBlock.max(1) as u32, Ordering::Relaxed);
        self.engine.set_sample_rate(setup.sampleRate as f32);
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid ProcessData for this call.
        let data = unsafe { &*data };
        let n = data.numSamples.max(0) as usize;
        if n == 0
            || data.numInputs < 1
            || data.numOutputs < 1
            || data.inputs.is_null()
            || data.outputs.is_null()
        {
            return kResultOk;
        }
        // SAFETY: `inputs`/`outputs` point to at least one bus each (checked above).
        let (inb, outb) = unsafe { (&*data.inputs, &*data.outputs) };
        let channels = inb.numChannels.min(outb.numChannels).max(0) as usize;
        if channels == 0 {
            return kResultOk;
        }
        // SAFETY: for 32-bit processing the host fills channelBuffers32 with `numChannels`
        // pointers, each to `numSamples` samples.
        let (ins, outs) = unsafe {
            let ip = inb.__field0.channelBuffers32;
            let op = outb.__field0.channelBuffers32;
            if ip.is_null() || op.is_null() {
                return kResultOk;
            }
            (
                std::slice::from_raw_parts(ip, channels),
                std::slice::from_raw_parts(op, channels),
            )
        };
        for c in 0..channels {
            if ins[c].is_null() || outs[c].is_null() {
                return kResultOk;
            }
            if ins[c] != outs[c] {
                // SAFETY: distinct host buffers of `n` samples each.
                unsafe { std::ptr::copy_nonoverlapping(ins[c], outs[c], n) };
            }
        }
        // Parameter changes from the host (automation): the last point of each queue.
        if !data.inputParameterChanges.is_null() {
            // SAFETY: a live IParameterChanges for this call.
            if let Some(changes) = unsafe { ComRef::from_raw(data.inputParameterChanges) } {
                // SAFETY: the queue pointers it hands out live for this call.
                unsafe {
                    for i in 0..changes.getParameterCount() {
                        let Some(q) = ComRef::from_raw(changes.getParameterData(i)) else {
                            continue;
                        };
                        let n = q.getPointCount();
                        let mut offset = 0;
                        let mut value = 0.0;
                        let p = Pid(q.getParameterId());
                        if n > 0
                            && q.getPoint(n - 1, &mut offset, &mut value) == kResultOk
                            && let Some(i) = p.info()
                        {
                            self.engine.set_from_host(p, i.denormalize(value));
                        }
                    }
                }
            }
        }
        // Never block the audio thread: if the processor is being swapped, pass through.
        let Ok(mut guard) = self.processor.try_lock() else {
            return kResultOk;
        };
        let Some(proc_) = guard.as_mut() else {
            return kResultOk;
        };
        // SAFETY: output buffers are valid for `n` samples and, when there are two, distinct.
        let left = unsafe { std::slice::from_raw_parts_mut(outs[0], n) };
        if channels >= 2 && outs[1] != outs[0] {
            // SAFETY: as above, the second channel.
            let right = unsafe { std::slice::from_raw_parts_mut(outs[1], n) };
            proc_.process(&mut [left, right]);
        } else {
            proc_.process(&mut [left]);
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> uint32 {
        self.engine.latency()
    }
}

impl IProcessContextRequirementsTrait for Verb {
    unsafe fn getProcessContextRequirements(&self) -> uint32 {
        // No transport or tempo needed.
        0
    }
}

impl IEditControllerTrait for Verb {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> int32 {
        params::count() as int32
    }

    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        let Some(i) = usize::try_from(index).ok().and_then(|i| TABLE.get(i)) else {
            return kInvalidArgument;
        };
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable ParameterInfo from the host.
        let info = unsafe { &mut *info };
        info.id = i.id.0;
        copy_wstring(&i.name, &mut info.title);
        copy_wstring(&i.short, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = if i.steps > 0 { i.steps as int32 - 1 } else { 0 };
        info.defaultNormalizedValue = i.normalize(i.default);
        info.unitId = 0;
        let mut flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        match i.id.decode() {
            Some(Which::G(G::Bypass)) => flags |= ParameterInfo_::ParameterFlags_::kIsBypass,
            Some(Which::G(G::Version)) => flags |= ParameterInfo_::ParameterFlags_::kIsList,
            _ if matches!(i.kind, Kind::Choice(_)) => {
                flags |= ParameterInfo_::ParameterFlags_::kIsList;
            }
            _ => {}
        }
        info.flags = flags;
        kResultOk
    }

    unsafe fn getParamStringByValue(
        &self,
        id: ParamID,
        value: ParamValue,
        string: *mut String128,
    ) -> tresult {
        let p = Pid(id);
        let Some(i) = p.info() else {
            return kInvalidArgument;
        };
        if string.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable String128 from the host.
        let out = unsafe { &mut *string };
        copy_wstring(&params::format(p, i.denormalize(value)), out);
        kResultOk
    }

    unsafe fn getParamValueByString(
        &self,
        id: ParamID,
        string: *mut TChar,
        value: *mut ParamValue,
    ) -> tresult {
        let p = Pid(id);
        let Some(i) = p.info() else {
            return kInvalidArgument;
        };
        if string.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: a NUL-terminated UTF-16 string from the host; read up to 128 units.
        let text = unsafe {
            let mut len = 0;
            while len < 128 && *string.add(len) != 0 {
                len += 1;
            }
            str_from_tchar(std::slice::from_raw_parts(string, len))
        };
        match params::parse(p, &text) {
            Some(v) => {
                // SAFETY: non-null out pointer from the host.
                unsafe { *value = i.normalize(v) };
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn normalizedParamToPlain(&self, id: ParamID, value: ParamValue) -> ParamValue {
        Pid(id).info().map_or(value, |i| i.denormalize(value))
    }

    unsafe fn plainParamToNormalized(&self, id: ParamID, value: ParamValue) -> ParamValue {
        Pid(id).info().map_or(value, |i| i.normalize(value))
    }

    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        let p = Pid(id);
        p.info()
            .map_or(0.0, |i| i.normalize(self.engine.params().get(p)))
    }

    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        let p = Pid(id);
        let Some(i) = p.info() else {
            return kInvalidArgument;
        };
        self.engine.set_from_host(p, i.denormalize(value));
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        // SAFETY: a live handler (or null); to_com_ptr takes our own reference.
        let h = unsafe { ComRef::from_raw(handler) }.map(|r| HostHandler(r.to_com_ptr()));
        if let Ok(mut slot) = self.handler.lock() {
            *slot = h;
        }
        kResultOk
    }

    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if name.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: the host passes a NUL-terminated view name.
        if unsafe { CStr::from_ptr(name) }.to_bytes() != b"editor" {
            return std::ptr::null_mut();
        }
        ComWrapper::new(View::new(self.engine.clone(), self.handler.clone()))
            .to_com_ptr::<IPlugView>()
            .map_or(std::ptr::null_mut(), |p| p.into_raw())
    }
}

/// The editor view.
struct View {
    #[cfg_attr(not(windows), allow(dead_code))]
    engine: Arc<Engine>,
    #[cfg_attr(not(windows), allow(dead_code))]
    handler: Arc<Mutex<Option<HostHandler>>>,
    window: RefCell<Option<egui_baseview::baseview::Window>>,
    size: Cell<(f64, f64)>,
    scale: Cell<f64>,
}

impl Class for View {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

impl View {
    fn new(engine: Arc<Engine>, handler: Arc<Mutex<Option<HostHandler>>>) -> Self {
        Self {
            engine,
            handler,
            window: RefCell::new(None),
            size: Cell::new(DEFAULT_SIZE),
            scale: Cell::new(1.0),
        }
    }

    fn physical(&self) -> (i32, i32) {
        let (w, h) = self.size.get();
        let s = self.scale.get();
        ((w * s).round() as i32, (h * s).round() as i32)
    }

    fn close(&self) {
        if let Some(w) = self.window.borrow_mut().take() {
            w.close();
        }
    }
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, r#type: FIDString) -> tresult {
        if r#type.is_null() {
            return kResultFalse;
        }
        // SAFETY: NUL-terminated platform type string from the host.
        let t = unsafe { CStr::from_ptr(r#type) };
        if cfg!(windows) && t.to_bytes() == b"HWND" {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, r#type: FIDString) -> tresult {
        // SAFETY: as in isPlatformTypeSupported.
        if unsafe { self.isPlatformTypeSupported(r#type) } != kResultTrue {
            return kResultFalse;
        }
        self.close();
        #[cfg(windows)]
        {
            let Some(hwnd) = std::num::NonZeroIsize::new(parent as isize) else {
                return kInvalidArgument;
            };
            // Each frame: if the mode or lookahead (and so the latency) changed, tell the host.
            let handler = self.handler.clone();
            let hook: crate::editor::FrameHook = Box::new(move |engine: &Engine| {
                if engine.take_latency_changed()
                    && let Ok(h) = handler.lock()
                    && let Some(HostHandler(h)) = h.as_ref()
                {
                    // SAFETY: see HostHandler: the UI thread, a live handler.
                    unsafe {
                        h.restartComponent(RestartFlags_::kLatencyChanged);
                    }
                }
            });
            match crate::editor::open(
                &crate::editor::RawParent(hwnd),
                self.engine.clone(),
                self.size.get(),
                Some(hook),
            ) {
                Ok(w) => {
                    *self.window.borrow_mut() = Some(w);
                    kResultOk
                }
                Err(e) => {
                    log::error!("SolPlugs Verb: couldn't open the editor: {e}");
                    kResultFalse
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = parent;
            kResultFalse
        }
    }

    unsafe fn removed(&self) -> tresult {
        self.close();
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyDown(&self, _key: char16, _key_code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _key: char16, _key_code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        let (w, h) = self.physical();
        // SAFETY: non-null, writable ViewRect from the host.
        unsafe {
            *size = ViewRect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            }
        };
        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null ViewRect from the host, read only.
        let r = unsafe { *new_size };
        let s = self.scale.get();
        let logical = (
            (f64::from(r.right - r.left) / s).max(MIN_SIZE.0),
            (f64::from(r.bottom - r.top) / s).max(MIN_SIZE.1),
        );
        self.size.set(logical);
        if let Some(w) = self.window.borrow().as_ref() {
            let _ = w.resize(egui_baseview::baseview::dpi::LogicalSize {
                width: logical.0,
                height: logical.1,
            });
        }
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, _frame: *mut IPlugFrame) -> tresult {
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }

    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        let s = self.scale.get();
        // SAFETY: non-null, writable ViewRect from the host.
        let r = unsafe { &mut *rect };
        let min_w = (MIN_SIZE.0 * s).round() as i32;
        let min_h = (MIN_SIZE.1 * s).round() as i32;
        if r.right - r.left < min_w {
            r.right = r.left + min_w;
        }
        if r.bottom - r.top < min_h {
            r.bottom = r.top + min_h;
        }
        kResultTrue
    }
}

impl IPlugViewContentScaleSupportTrait for View {
    unsafe fn setContentScaleFactor(
        &self,
        factor: IPlugViewContentScaleSupport_::ScaleFactor,
    ) -> tresult {
        if factor.is_finite() && factor > 0.0 {
            self.scale.set(f64::from(factor));
        }
        kResultOk
    }
}

impl Drop for View {
    fn drop(&mut self) {
        self.close();
    }
}

/// The factory the host gets from `GetPluginFactory`.
struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory3,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable PFactoryInfo from the host.
        let info = unsafe { &mut *info };
        copy_cstring(crate::VENDOR, &mut info.vendor);
        copy_cstring(crate::URL, &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> int32 {
        1
    }

    unsafe fn getClassInfo(&self, index: int32, info: *mut PClassInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable PClassInfo from the host.
        let info = unsafe { &mut *info };
        info.cid = CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring("Audio Module Class", &mut info.category);
        copy_cstring(crate::NAME, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: `cid` points to a 16-byte TUID.
        if unsafe { *(cid as *const TUID) } != CID {
            return kInvalidArgument;
        }
        let Some(instance) = ComWrapper::new(Verb::new()).to_com_ptr::<FUnknown>() else {
            return kResultFalse;
        };
        let ptr = instance.as_ptr();
        // SAFETY: `ptr` is a live FUnknown we own; queryInterface adds the reference the host keeps.
        unsafe { ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: int32, info: *mut PClassInfo2) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable PClassInfo2 from the host.
        let info = unsafe { &mut *info };
        info.cid = CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring("Audio Module Class", &mut info.category);
        copy_cstring(crate::NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring("Fx|Reverb", &mut info.subCategories);
        copy_cstring(crate::VENDOR, &mut info.vendor);
        copy_cstring(crate::VERSION, &mut info.version);
        copy_cstring("VST 3.7", &mut info.sdkVersion);
        kResultOk
    }
}

impl IPluginFactory3Trait for Factory {
    unsafe fn getClassInfoUnicode(&self, index: int32, info: *mut PClassInfoW) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null, writable PClassInfoW from the host.
        let info = unsafe { &mut *info };
        info.cid = CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring("Audio Module Class", &mut info.category);
        copy_wstring(crate::NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring("Fx|Reverb", &mut info.subCategories);
        copy_wstring(crate::VENDOR, &mut info.vendor);
        copy_wstring(crate::VERSION, &mut info.version);
        copy_wstring("VST 3.7", &mut info.sdkVersion);
        kResultOk
    }

    unsafe fn setHostContext(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_handle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

/// The VST3 entry point.
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .map_or(std::ptr::null_mut(), |p| p.into_raw())
}
