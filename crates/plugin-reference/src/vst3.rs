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

use solplugs_ref_app::engine::Engine;
use solplugs_ref_app::processor::Processor;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComRef, ComWrapper, uid};

use crate::editor::{DEFAULT_SIZE, MIN_SIZE};

/// Class id of the single component.
const CID: TUID = uid(0xDDA8_F6BC, 0x35D5_40AD, 0xA370_C318, 0xA8D2_D555);
/// Largest state we accept from the host.
const MAX_STATE: usize = 1 << 20;

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

/// The plugin object.
struct Reference {
    engine: Arc<Engine>,
    processor: Mutex<Option<Processor>>,
    sample_rate: AtomicU64,
    max_block: AtomicU32,
    scratch: Mutex<Vec<f32>>,
}

impl Class for Reference {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IProcessContextRequirements,
        IEditController,
    );
}

impl Reference {
    fn new() -> Self {
        Self {
            engine: Arc::new(Engine::new()),
            processor: Mutex::new(None),
            sample_rate: AtomicU64::new(48_000f64.to_bits()),
            max_block: AtomicU32::new(4096),
            scratch: Mutex::new(Vec::new()),
        }
    }
}

impl IPluginBaseTrait for Reference {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        if let Ok(mut p) = self.processor.lock() {
            *p = None;
        }
        kResultOk
    }
}

impl IComponentTrait for Reference {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        // Single component: the host queries IEditController from this object.
        kNotImplemented
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, _dir: BusDirection) -> int32 {
        i32::from(media_type == MediaTypes_::kAudio as MediaType)
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        if media_type != MediaTypes_::kAudio as MediaType || index != 0 || bus.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a valid, writable BusInfo (checked non-null above).
        let bus = unsafe { &mut *bus };
        bus.mediaType = MediaTypes_::kAudio as MediaType;
        bus.direction = dir;
        bus.channelCount = 2;
        copy_wstring(
            if dir == BusDirections_::kInput as BusDirection {
                "Main In"
            } else {
                "Main Out"
            },
            &mut bus.name,
        );
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
            *p = Some(Processor::new(self.engine.bridge(), sr, max));
            if let Ok(mut s) = self.scratch.lock() {
                s.clear();
                s.resize(max.clamp(32, 1 << 16), 0.0);
            }
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
        self.engine.load_state(&data);
        kResultOk
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

impl IAudioProcessorTrait for Reference {
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
        // SAFETY: the host passes arrays of `num_ins` / `num_outs` (both 1, checked) arrangements.
        let (i, o) = unsafe { (*inputs, *outputs) };
        if i == SpeakerArr::kStereo && o == SpeakerArr::kStereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getBusArrangement(
        &self,
        _dir: BusDirection,
        index: int32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if index != 0 || arr.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: non-null out pointer from the host.
        unsafe { *arr = SpeakerArr::kStereo };
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
        0
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
            proc_.process(left, right);
        } else if let Ok(mut s) = self.scratch.try_lock() {
            let m = n.min(s.len());
            s[..m].copy_from_slice(&left[..m]);
            proc_.process(&mut left[..m], &mut s[..m]);
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> uint32 {
        0
    }
}

impl IProcessContextRequirementsTrait for Reference {
    unsafe fn getProcessContextRequirements(&self) -> uint32 {
        0
    }
}

impl IEditControllerTrait for Reference {
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
        0
    }

    unsafe fn getParameterInfo(&self, _index: int32, _info: *mut ParameterInfo) -> tresult {
        kInvalidArgument
    }

    unsafe fn getParamStringByValue(
        &self,
        _id: ParamID,
        _value: ParamValue,
        _string: *mut String128,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value: *mut ParamValue,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn normalizedParamToPlain(&self, _id: ParamID, value: ParamValue) -> ParamValue {
        value
    }

    unsafe fn plainParamToNormalized(&self, _id: ParamID, value: ParamValue) -> ParamValue {
        value
    }

    unsafe fn getParamNormalized(&self, _id: ParamID) -> ParamValue {
        0.0
    }

    unsafe fn setParamNormalized(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kInvalidArgument
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
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
        ComWrapper::new(View::new(self.engine.clone()))
            .to_com_ptr::<IPlugView>()
            .map_or(std::ptr::null_mut(), |p| p.into_raw())
    }
}

/// The editor view.
struct View {
    #[cfg_attr(not(windows), allow(dead_code))]
    engine: Arc<Engine>,
    window: RefCell<Option<egui_baseview::baseview::Window>>,
    size: Cell<(f64, f64)>,
    scale: Cell<f64>,
}

impl Class for View {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

impl View {
    fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
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
            match crate::editor::open(
                &crate::editor::RawParent(hwnd),
                self.engine.clone(),
                self.size.get(),
            ) {
                Ok(w) => {
                    *self.window.borrow_mut() = Some(w);
                    kResultOk
                }
                Err(e) => {
                    log::error!("SolPlugs Reference: couldn't open the editor: {e}");
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
    type Interfaces = (IPluginFactory2,);
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
        let Some(instance) = ComWrapper::new(Reference::new()).to_com_ptr::<FUnknown>() else {
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
        copy_cstring("Fx|Analyzer", &mut info.subCategories);
        copy_cstring(crate::VENDOR, &mut info.vendor);
        copy_cstring(crate::VERSION, &mut info.version);
        copy_cstring("VST 3.7", &mut info.sdkVersion);
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
