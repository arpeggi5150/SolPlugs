//! The CLAP plugin, through clack's safe wrappers.

use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiResizeHints, GuiSize, PluginGui, PluginGuiImpl, Window,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::params::{
    HostParams, ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, ParamRescanFlags,
    PluginAudioProcessorParams, PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::Pckn;
use clack_plugin::events::event_types::{
    ParamGestureBeginEvent, ParamGestureEndEvent, ParamValueEvent, TransportFlags,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::plugin::features;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use clack_plugin::utils::Cookie;
use solplugs_delay_app::engine::{Engine, HostEvent, Processor};
use solplugs_delay_app::params::{self, G, Kind, Pid, TABLE, Which};

use crate::editor::{self, DEFAULT_SIZE, MIN_SIZE};

/// The plugin type.
pub struct DelayClap;

/// Shared between threads.
pub struct Shared {
    engine: Arc<Engine>,
    /// UI edits waiting to be sent to the host as events.
    outbox: Arc<Mutex<Vec<HostEvent>>>,
}

impl PluginShared<'_> for Shared {}

/// Main-thread state: the editor window.
pub struct MainThread<'a> {
    shared: &'a Shared,
    host: HostMainThreadHandle<'a>,
    host_params: Option<HostParams>,
    window: RefCell<Option<egui_baseview::baseview::Window>>,
    scale: Cell<f64>,
    size: Cell<(f64, f64)>,
}

impl<'a> PluginMainThread<'a, Shared> for MainThread<'a> {}

/// Audio-thread state.
pub struct AudioProcessor<'a> {
    processor: Processor,
    shared: &'a Shared,
    host: HostAudioProcessorHandle<'a>,
    latency: u32,
    /// The sidechain input, copied out of the host's buffers.
    sc: [Vec<f32>; 2],
}

impl Plugin for DelayClap {
    type AudioProcessor<'a> = AudioProcessor<'a>;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginGui>()
            .register::<PluginState>()
            .register::<PluginLatency>()
            .register::<PluginParams>();
    }
}

impl DefaultPluginFactory for DelayClap {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new(crate::CLAP_ID, crate::NAME)
            .with_vendor(crate::VENDOR)
            .with_url(crate::URL)
            .with_version(crate::VERSION)
            .with_description(
                "Three delays in one: Clean, Tape and Analog, with tempo-sync, tap tempo, ping-pong, reverse and ducking.",
            )
            .with_features([
                features::AUDIO_EFFECT,
                features::DELAY,
                features::STEREO,
                features::MONO,
            ])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<Shared, PluginError> {
        let engine = Arc::new(Engine::new());
        let outbox: Arc<Mutex<Vec<HostEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let o = outbox.clone();
        engine.set_listener(Some(Arc::new(move |e| {
            if let Ok(mut q) = o.lock() {
                q.push(e);
            }
        })));
        Ok(Shared { engine, outbox })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a Shared,
    ) -> Result<MainThread<'a>, PluginError> {
        Ok(MainThread {
            shared,
            host_params: host.get_extension(),
            host,
            window: RefCell::new(None),
            scale: Cell::new(1.0),
            size: Cell::new(DEFAULT_SIZE),
        })
    }
}

fn apply_input_events(engine: &Engine, events: &InputEvents) {
    for e in events {
        if let Some(CoreEventSpace::ParamValue(v)) = e.as_core_event()
            && let Some(id) = v.param_id()
            && Pid(id.get()).info().is_some()
        {
            engine.set_from_host(Pid(id.get()), v.value());
        }
    }
}

fn send_outbox(shared: &Shared, out: &mut OutputEvents) {
    // Never block the audio thread: if the UI holds the lock, send next block.
    let Ok(mut q) = shared.outbox.try_lock() else {
        return;
    };
    for e in q.drain(..) {
        let r = match e {
            HostEvent::Begin(p) => out.try_push(ParamGestureBeginEvent::new(0, ClapId::new(p.0))),
            HostEvent::Value(p, v) => out.try_push(ParamValueEvent::new(
                0,
                ClapId::new(p.0),
                Pckn::match_all(),
                v,
            )),
            HostEvent::End(p) => out.try_push(ParamGestureEndEvent::new(0, ClapId::new(p.0))),
        };
        let _ = r;
    }
}

impl<'a> PluginAudioProcessor<'a, Shared, MainThread<'a>> for AudioProcessor<'a> {
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        _main: &MainThread<'a>,
        shared: &'a Shared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let n = config.max_frames_count.max(1) as usize;
        let processor = shared
            .engine
            .processor(config.sample_rate, config.max_frames_count as usize);
        let latency = processor.latency();
        Ok(Self {
            processor,
            shared,
            host,
            latency,
            sc: [vec![0.0; n], vec![0.0; n]],
        })
    }

    fn process(
        &mut self,
        process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        apply_input_events(&self.shared.engine, events.input);
        send_outbox(self.shared, events.output);
        // The song tempo, when the host sends one.
        let bpm = process
            .transport
            .filter(|t| t.flags.contains(TransportFlags::HAS_TEMPO))
            .map(|t| t.tempo);
        let n = audio.frames_count() as usize;
        let has_sc = self.copy_sidechain(&audio, n);
        let Some(mut port) = audio.port_pair(0) else {
            return Ok(ProcessStatus::Continue);
        };
        let Some(mut ch) = port.channels()?.into_f32() else {
            return Ok(ProcessStatus::Continue);
        };
        fn out(pair: ChannelPair<'_, f32>) -> Option<&mut [f32]> {
            match pair {
                ChannelPair::InputOutput(i, o) => {
                    o.copy_from_slice(i);
                    Some(o)
                }
                ChannelPair::InPlace(b) => Some(b),
                ChannelPair::OutputOnly(o) => {
                    o.fill(0.0);
                    Some(o)
                }
                ChannelPair::InputOnly(_) => None,
            }
        }
        let left = ch.channel_pair(0).and_then(out);
        let right = if ch.channel_pair_count() > 1 {
            ch.channel_pair(1).and_then(out)
        } else {
            None
        };
        let [a, b] = &self.sc;
        let key = has_sc.then(|| [&a[..n.min(a.len())], &b[..n.min(b.len())]]);
        match (left, right) {
            (Some(l), Some(r)) => self.processor.process(&mut [l, r], key, bpm),
            (Some(l), None) => self.processor.process(&mut [l], key, bpm),
            _ => {}
        }
        if self.processor.latency() != self.latency {
            // The version changed the latency: the host restarts us (deactivate,
            // activate) and reads the new latency on activation.
            self.latency = self.processor.latency();
            self.host.shared().request_restart();
        }
        // Always continue: the repeats ring on after the input stops.
        Ok(ProcessStatus::Continue)
    }
}

impl AudioProcessor<'_> {
    /// Copies the sidechain port (inputs 3/4) into `self.sc`. False when the host gives
    /// none.
    fn copy_sidechain(&mut self, audio: &Audio<'_>, n: usize) -> bool {
        let Some(port) = audio.input_port(1) else {
            return false;
        };
        let Some(ch) = port.channels().ok().and_then(|c| c.into_f32()) else {
            return false;
        };
        let Some(l) = ch.channel(0) else {
            return false;
        };
        let r = ch.channel(1).unwrap_or(l);
        let n = n.min(self.sc[0].len()).min(l.len()).min(r.len());
        self.sc[0][..n].copy_from_slice(&l[..n]);
        self.sc[1][..n].copy_from_slice(&r[..n]);
        true
    }
}

impl PluginAudioProcessorParams for AudioProcessor<'_> {
    fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        apply_input_events(&self.shared.engine, input);
        send_outbox(self.shared, output);
    }
}

fn flags(id: Pid) -> ParamInfoFlags {
    let mut f = ParamInfoFlags::IS_AUTOMATABLE;
    if id.info().is_some_and(|i| i.steps > 0) {
        f |= ParamInfoFlags::IS_STEPPED;
    }
    match id.decode() {
        Some(Which::G(G::Bypass)) => f |= ParamInfoFlags::IS_BYPASS,
        Some(Which::G(G::Version)) => f |= ParamInfoFlags::IS_ENUM,
        _ if id.info().is_some_and(|i| matches!(i.kind, Kind::Choice(_))) => {
            f |= ParamInfoFlags::IS_ENUM
        }
        _ => {}
    }
    f
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&self) -> u32 {
        params::count() as u32
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        let Some(i) = TABLE.get(index as usize) else {
            return;
        };
        info.set(&ParamInfo {
            id: ClapId::new(i.id.0),
            flags: flags(i.id),
            cookie: Cookie::empty(),
            name: i.name.as_bytes(),
            module: i.group.as_bytes(),
            min_value: i.min,
            max_value: i.max,
            default_value: i.default,
        });
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        let p = Pid(id.get());
        p.info().map(|_| self.shared.engine.params().get(p))
    }

    fn value_to_text(
        &self,
        id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        let p = Pid(id.get());
        if p.info().is_none() {
            return Err(std::fmt::Error);
        }
        std::fmt::Write::write_str(writer, &params::format(p, value))
    }

    fn text_to_value(&self, id: ClapId, text: &CStr) -> Option<f64> {
        let p = Pid(id.get());
        p.info()?;
        params::parse(p, text.to_str().ok()?)
    }

    fn flush(&self, input: &InputEvents, output: &mut OutputEvents) {
        apply_input_events(&self.shared.engine, input);
        send_outbox(self.shared, output);
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        // Inputs: main (1/2) and the external sidechain (3/4).
        if is_input { 2 } else { 1 }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        match (index, is_input) {
            (0, _) => writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: if is_input { b"Main In" } else { b"Main Out" },
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: Some(ClapId::new(0)),
            }),
            (1, true) => writer.set(&AudioPortInfo {
                id: ClapId::new(1),
                name: b"Sidechain",
                channel_count: 2,
                flags: AudioPortFlags::empty(),
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            }),
            _ => {}
        }
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.engine.latency()
    }
}

impl PluginStateImpl for MainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        output
            .write_all(&self.shared.engine.save_state())
            .map_err(|_| PluginError::Message("couldn't save state"))
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut buf = Vec::new();
        input
            .take(1 << 30)
            .read_to_end(&mut buf)
            .map_err(|_| PluginError::Message("couldn't read state"))?;
        if self.shared.engine.load_state(&buf) {
            // The values changed under the host: ask it to read them again.
            if let Some(hp) = &self.host_params {
                hp.rescan(&self.host, ParamRescanFlags::VALUES);
            }
            Ok(())
        } else {
            Err(PluginError::Message("not a SolPlugs Delay state"))
        }
    }
}

impl PluginGuiImpl for MainThread<'_> {
    fn is_api_supported(&self, configuration: GuiConfiguration) -> bool {
        configuration.api_type
            == GuiApiType::default_for_current_platform().unwrap_or(GuiApiType::WIN32)
            && !configuration.is_floating
    }

    fn get_preferred_api(&self) -> Option<GuiConfiguration<'_>> {
        Some(GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        })
    }

    fn create(&self, _configuration: GuiConfiguration) -> Result<(), PluginError> {
        Ok(())
    }

    fn destroy(&self) {
        if let Some(w) = self.window.borrow_mut().take() {
            w.close();
        }
    }

    fn set_scale(&self, scale: f64) -> Result<(), PluginError> {
        if scale.is_finite() && scale > 0.0 {
            self.scale.set(scale);
        }
        Ok(())
    }

    fn get_size(&self) -> Option<GuiSize> {
        let (w, h) = self.size.get();
        let s = self.scale.get();
        Some(GuiSize {
            width: (w * s) as u32,
            height: (h * s) as u32,
        })
    }

    fn can_resize(&self) -> bool {
        true
    }

    fn get_resize_hints(&self) -> Option<GuiResizeHints> {
        Some(GuiResizeHints {
            can_resize_horizontally: true,
            can_resize_vertically: true,
            strategy: clack_extensions::gui::AspectRatioStrategy::Disregard,
        })
    }

    fn adjust_size(&self, size: GuiSize) -> Option<GuiSize> {
        let s = self.scale.get();
        Some(GuiSize {
            width: size.width.max((MIN_SIZE.0 * s) as u32),
            height: size.height.max((MIN_SIZE.1 * s) as u32),
        })
    }

    fn set_size(&self, size: GuiSize) -> Result<(), PluginError> {
        let s = self.scale.get();
        let logical = (
            (f64::from(size.width) / s).max(MIN_SIZE.0),
            (f64::from(size.height) / s).max(MIN_SIZE.1),
        );
        self.size.set(logical);
        if let Some(w) = self.window.borrow().as_ref() {
            let _ = w.resize(egui_baseview::baseview::dpi::LogicalSize {
                width: logical.0,
                height: logical.1,
            });
        }
        Ok(())
    }

    fn set_parent(&self, window: Window) -> Result<(), PluginError> {
        self.destroy();
        let w = editor::open(&window, self.shared.engine.clone(), self.size.get(), None)
            .map_err(|_| PluginError::Message("couldn't open the editor"))?;
        *self.window.borrow_mut() = Some(w);
        Ok(())
    }

    fn set_transient(&self, _window: Window) -> Result<(), PluginError> {
        Ok(())
    }

    fn show(&self) -> Result<(), PluginError> {
        if let Some(w) = self.window.borrow().as_ref() {
            let _ = w.show();
        }
        Ok(())
    }

    fn hide(&self) -> Result<(), PluginError> {
        if let Some(w) = self.window.borrow().as_ref() {
            let _ = w.hide();
        }
        Ok(())
    }
}

clack_export_entry!(SinglePluginEntry<DelayClap>);
