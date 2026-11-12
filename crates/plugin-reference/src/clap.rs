//! The CLAP plugin, through clack's safe wrappers.

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::sync::Arc;

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiResizeHints, GuiSize, PluginGui, PluginGuiImpl, Window,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::plugin::features;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use solplugs_ref_app::engine::Engine;
use solplugs_ref_app::processor::Processor;

use crate::editor::{self, DEFAULT_SIZE, MIN_SIZE};

/// The plugin type.
pub struct ReferenceClap;

/// Shared between threads: the engine.
pub struct Shared {
    engine: Arc<Engine>,
}

impl PluginShared<'_> for Shared {}

/// Main-thread state: the editor window.
pub struct MainThread<'a> {
    shared: &'a Shared,
    window: RefCell<Option<egui_baseview::baseview::Window>>,
    scale: Cell<f64>,
    size: Cell<(f64, f64)>,
}

impl<'a> PluginMainThread<'a, Shared> for MainThread<'a> {}

/// Audio-thread state.
pub struct AudioProcessor {
    processor: Processor,
    scratch: Vec<f32>,
}

impl Plugin for ReferenceClap {
    type AudioProcessor<'a> = AudioProcessor;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginGui>()
            .register::<PluginState>()
            .register::<PluginLatency>();
    }
}

impl DefaultPluginFactory for ReferenceClap {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new(crate::CLAP_ID, crate::NAME)
            .with_vendor(crate::VENDOR)
            .with_url(crate::URL)
            .with_version(crate::VERSION)
            .with_description(
                "Reference track analyzer: loudness-matched A/B, DIFF, PARTS, group referencing.",
            )
            .with_features([
                features::AUDIO_EFFECT,
                features::ANALYZER,
                features::UTILITY,
                features::STEREO,
            ])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<Shared, PluginError> {
        Ok(Shared {
            engine: Arc::new(Engine::new()),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a Shared,
    ) -> Result<MainThread<'a>, PluginError> {
        Ok(MainThread {
            shared,
            window: RefCell::new(None),
            scale: Cell::new(1.0),
            size: Cell::new(DEFAULT_SIZE),
        })
    }
}

impl<'a> PluginAudioProcessor<'a, Shared, MainThread<'a>> for AudioProcessor {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main: &MainThread<'a>,
        shared: &'a Shared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let max = config.max_frames_count as usize;
        Ok(Self {
            processor: Processor::new(shared.engine.bridge(), config.sample_rate, max),
            scratch: vec![0.0; max.clamp(32, 1 << 16)],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let Some(mut port) = audio.port_pair(0) else {
            return Ok(ProcessStatus::ContinueIfNotQuiet);
        };
        let Some(mut ch) = port.channels()?.into_f32() else {
            return Ok(ProcessStatus::ContinueIfNotQuiet);
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
        match (left, right) {
            (Some(l), Some(r)) => self.processor.process(l, r),
            (Some(l), None) => {
                // Mono: analyse and A/B the one channel as both sides.
                let n = l.len().min(self.scratch.len());
                self.scratch[..n].copy_from_slice(&l[..n]);
                self.processor.process(&mut l[..n], &mut self.scratch[..n]);
            }
            _ => {}
        }
        Ok(ProcessStatus::ContinueIfNotQuiet)
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: if is_input { b"Main In" } else { b"Main Out" },
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: Some(ClapId::new(0)),
            });
        }
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        0
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
            .take(1 << 20)
            .read_to_end(&mut buf)
            .map_err(|_| PluginError::Message("couldn't read state"))?;
        self.shared.engine.load_state(&buf);
        Ok(())
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
        let w = editor::open(&window, self.shared.engine.clone(), self.size.get())
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

clack_export_entry!(SinglePluginEntry<ReferenceClap>);
