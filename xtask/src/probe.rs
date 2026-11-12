//! `cargo xtask probe-vst3 <binary>`: a minimal VST3 host that walks the calls a DAW
//! makes while scanning and instantiating a plugin, and prints each result.
//! It is a diagnostic for "the host found the file but won't list it".
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::path::Path;
use std::ptr;

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, Interface};

fn cstr(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn wstr(buf: &[TChar]) -> String {
    let units: Vec<u16> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u16)
        .collect();
    String::from_utf16_lossy(&units)
}

fn check(step: &str, r: tresult) -> Result<(), String> {
    let ok = r == kResultOk;
    println!(
        "  {step}: {r}{}",
        if ok { "" } else { "  <-- not kResultOk" }
    );
    if ok {
        Ok(())
    } else {
        Err(format!("{step} returned {r}"))
    }
}

pub fn run(path: &Path) -> Result<(), String> {
    println!("probe: {}", path.display());
    // SAFETY: loading a plugin binary the user pointed us at; this is the purpose of the tool.
    let lib = unsafe { libloading::Library::new(path) }.map_err(|e| format!("load: {e}"))?;

    #[cfg(windows)]
    {
        // SAFETY: symbol signature per the VST3 module spec.
        if let Ok(init) = unsafe { lib.get::<extern "system" fn() -> bool>(b"InitDll") } {
            println!("  InitDll: {}", init());
        }
    }

    // SAFETY: symbol signature per the VST3 module spec.
    let get_factory =
        unsafe { lib.get::<extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory") }
            .map_err(|e| format!("GetPluginFactory: {e}"))?;
    // SAFETY: the factory returns an owned reference (or null).
    let factory =
        unsafe { ComPtr::from_raw(get_factory()) }.ok_or("GetPluginFactory returned null")?;

    let result = scan_and_run(&factory);
    drop(factory);

    #[cfg(windows)]
    {
        // SAFETY: symbol signature per the VST3 module spec.
        if let Ok(exit) = unsafe { lib.get::<extern "system" fn() -> bool>(b"ExitDll") } {
            println!("  ExitDll: {}", exit());
        }
    }
    drop(lib);
    match &result {
        Ok(()) => println!("probe: OK"),
        Err(e) => println!("probe: FAILED: {e}"),
    }
    result
}

fn scan_and_run(factory: &ComPtr<IPluginFactory>) -> Result<(), String> {
    // SAFETY (whole fn): all calls go through vtables of live COM objects with
    // correctly sized out-parameters, as a host would make them.
    unsafe {
        let mut fi: PFactoryInfo = std::mem::zeroed();
        check("getFactoryInfo", factory.getFactoryInfo(&mut fi))?;
        println!(
            "    vendor={:?} url={:?} flags={:#x}",
            cstr(&fi.vendor),
            cstr(&fi.url),
            fi.flags
        );

        let n = factory.countClasses();
        println!("  countClasses: {n}");
        let mut cid: TUID = [0; 16];
        for i in 0..n {
            let mut ci: PClassInfo = std::mem::zeroed();
            check("getClassInfo", factory.getClassInfo(i, &mut ci))?;
            println!(
                "    [{i}] category={:?} name={:?}",
                cstr(&ci.category),
                cstr(&ci.name)
            );
            if cstr(&ci.category) == "Audio Module Class" {
                cid = ci.cid;
            }
        }
        match factory.cast::<IPluginFactory2>() {
            Some(f2) => {
                let mut ci: PClassInfo2 = std::mem::zeroed();
                check("getClassInfo2", f2.getClassInfo2(0, &mut ci))?;
                println!(
                    "    sub={:?} vendor={:?} version={:?} sdk={:?}",
                    cstr(&ci.subCategories),
                    cstr(&ci.vendor),
                    cstr(&ci.version),
                    cstr(&ci.sdkVersion)
                );
            }
            None => println!("  IPluginFactory2: not supported"),
        }
        match factory.cast::<IPluginFactory3>() {
            Some(f3) => {
                let mut ci: PClassInfoW = std::mem::zeroed();
                check("getClassInfoUnicode", f3.getClassInfoUnicode(0, &mut ci))?;
                println!(
                    "    name={:?} vendor={:?}",
                    wstr(&ci.name),
                    wstr(&ci.vendor)
                );
                check("setHostContext", f3.setHostContext(ptr::null_mut()))?;
            }
            None => println!("  IPluginFactory3: not supported"),
        }

        let mut obj: *mut c_void = ptr::null_mut();
        check(
            "createInstance(IComponent)",
            factory.createInstance(
                cid.as_ptr(),
                IComponent::IID.as_ptr() as *const c_char,
                &mut obj,
            ),
        )?;
        let comp =
            ComPtr::<IComponent>::from_raw(obj as *mut IComponent).ok_or("null component")?;
        check("component.initialize", comp.initialize(ptr::null_mut()))?;

        let mut ctrl_cid: TUID = [0; 16];
        let r = comp.getControllerClassId(&mut ctrl_cid);
        println!("  getControllerClassId: {r} (non-ok means single component)");
        let ctrl = comp
            .cast::<IEditController>()
            .ok_or("component has no IEditController")?;
        println!("  IEditController on component: yes");
        println!("  getParameterCount: {}", ctrl.getParameterCount());

        for (dir, label) in [
            (BusDirections_::kInput, "in"),
            (BusDirections_::kOutput, "out"),
        ] {
            let d = dir as BusDirection;
            let count = comp.getBusCount(MediaTypes_::kAudio as MediaType, d);
            println!("  audio buses {label}: {count}");
            for b in 0..count {
                let mut bi: BusInfo = std::mem::zeroed();
                check(
                    "getBusInfo",
                    comp.getBusInfo(MediaTypes_::kAudio as MediaType, d, b, &mut bi),
                )?;
                println!(
                    "    {:?} ch={} flags={}",
                    wstr(&bi.name),
                    bi.channelCount,
                    bi.flags
                );
            }
            println!(
                "  event buses {label}: {}",
                comp.getBusCount(MediaTypes_::kEvent as MediaType, d)
            );
        }

        let proc = comp
            .cast::<IAudioProcessor>()
            .ok_or("component has no IAudioProcessor")?;
        let mut ins = [SpeakerArr::kStereo];
        let mut outs = [SpeakerArr::kStereo];
        check(
            "setBusArrangements",
            proc.setBusArrangements(ins.as_mut_ptr(), 1, outs.as_mut_ptr(), 1),
        )?;
        check(
            "canProcessSampleSize(32)",
            proc.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as int32),
        )?;
        println!("  getLatencySamples: {}", proc.getLatencySamples());
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: 512,
            sampleRate: 48_000.0,
        };
        check("setupProcessing", proc.setupProcessing(&mut setup))?;
        check("setActive(1)", comp.setActive(1))?;
        let r = proc.setProcessing(1);
        println!("  setProcessing(1): {r}");

        let frames = 512usize;
        let mut max_err = 0.0f32;
        for block in 0..200 {
            let mut inl: Vec<f32> = (0..frames)
                .map(|i| ((block * frames + i) as f32 * 0.013).sin() * 0.5)
                .collect();
            let mut inr: Vec<f32> = inl.iter().map(|x| -x * 0.7).collect();
            let expect_l = inl.clone();
            let expect_r = inr.clone();
            let mut outl = vec![0.0f32; frames];
            let mut outr = vec![0.0f32; frames];
            let mut in_ptrs = [inl.as_mut_ptr(), inr.as_mut_ptr()];
            let mut out_ptrs = [outl.as_mut_ptr(), outr.as_mut_ptr()];
            let mut inb = AudioBusBuffers {
                numChannels: 2,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: in_ptrs.as_mut_ptr(),
                },
            };
            let mut outb = AudioBusBuffers {
                numChannels: 2,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: out_ptrs.as_mut_ptr(),
                },
            };
            let mut data = ProcessData {
                processMode: ProcessModes_::kRealtime as int32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
                numSamples: frames as int32,
                numInputs: 1,
                numOutputs: 1,
                inputs: &mut inb,
                outputs: &mut outb,
                inputParameterChanges: ptr::null_mut(),
                outputParameterChanges: ptr::null_mut(),
                inputEvents: ptr::null_mut(),
                outputEvents: ptr::null_mut(),
                processContext: ptr::null_mut(),
            };
            let r = proc.process(&mut data);
            if r != kResultOk {
                return Err(format!("process returned {r} on block {block}"));
            }
            for i in 0..frames {
                max_err = max_err
                    .max((outl[i] - expect_l[i]).abs())
                    .max((outr[i] - expect_r[i]).abs());
            }
        }
        println!("  process x200: ok, max |out - in| = {max_err}");

        // A null-buffer flush, which some hosts send.
        let mut flush = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: 0,
            numInputs: 0,
            numOutputs: 0,
            inputs: ptr::null_mut(),
            outputs: ptr::null_mut(),
            inputParameterChanges: ptr::null_mut(),
            outputParameterChanges: ptr::null_mut(),
            inputEvents: ptr::null_mut(),
            outputEvents: ptr::null_mut(),
            processContext: ptr::null_mut(),
        };
        println!("  process(flush): {}", proc.process(&mut flush));

        let view = ctrl.createView(c"editor".as_ptr());
        match ComPtr::<IPlugView>::from_raw(view) {
            Some(v) => {
                let mut rect: ViewRect = std::mem::zeroed();
                let r = v.getSize(&mut rect);
                println!(
                    "  createView: ok, getSize {r} -> {}x{}",
                    rect.right - rect.left,
                    rect.bottom - rect.top
                );
                #[cfg(windows)]
                println!(
                    "  isPlatformTypeSupported(HWND): {}",
                    v.isPlatformTypeSupported(c"HWND".as_ptr())
                );
            }
            None => println!("  createView: null  <-- no editor"),
        }

        println!("  setProcessing(0): {}", proc.setProcessing(0));
        check("setActive(0)", comp.setActive(0))?;
        check("terminate", comp.terminate())?;
        drop(proc);
        drop(ctrl);
        drop(comp);
    }
    Ok(())
}
