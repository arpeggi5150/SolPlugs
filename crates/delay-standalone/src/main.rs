//! SolPlugs Delay in its own window: the same engine and editor as the plugin, with
//! a live input or a looping WAV file running through it to the default output.
//!
//! `solplugs-delay [--file song.wav] [--version clean|tape|analog] [--preset N] [--stacked]`
//! Drop a WAV on the window to loop it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod source;

use std::path::Path;
use std::sync::Arc;

use egui::RichText;
use solplugs_delay_app::engine::Engine;
use solplugs_delay_app::ui::theme::{self, Palette};
use solplugs_delay_app::ui::{DEFAULT_SIZE, MIN_SIZE, UiLocal, show};
use solplugs_delay_core::Version;
use source::{Source, Sources};

struct App {
    engine: Arc<Engine>,
    sources: Arc<Sources>,
    local: UiLocal,
    status: String,
    _audio: Option<audio::Output>,
}

impl App {
    fn load(&mut self, path: &Path) {
        match source::read_wav(path) {
            Ok(file) => {
                self.status = format!(
                    "{} ({:.1} s)",
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                    file.seconds()
                );
                self.sources.set_file(file);
                self.sources.select(Source::File);
            }
            Err(e) => self.status = format!("Couldn't open {}: {e}", path.display()),
        }
    }

    fn source_bar(&mut self, ui: &mut egui::Ui) {
        let pal = Palette::default();
        egui::Panel::top("dl-source")
            .frame(theme::bar_frame(&pal).fill(pal.card))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Source").color(pal.dim));
                    let cur = self.sources.selected();
                    for (s, label, help) in [
                        (Source::Off, "Off", "Silence."),
                        (Source::Live, "Live input", "The system's default input, through the delay to the default output. Use headphones."),
                        (Source::File, "File", "Loops the WAV you dropped on the window."),
                    ] {
                        let enabled = s != Source::Live || audio::HAS_INPUT;
                        let resp = ui.add_enabled(enabled, egui::Button::selectable(cur == s, label));
                        if resp.on_hover_text(help).clicked() {
                            self.sources.select(s);
                        }
                    }
                    ui.label(RichText::new(&self.status).color(pal.dim));
                });
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped: Vec<_> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(p) = dropped.first() {
            self.load(p);
        }
        self.source_bar(ui);
        show(ui, &self.engine, &mut self.local);
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let engine = Arc::new(Engine::new());
    if let Some(i) = arg(&args, "--preset").and_then(|s| s.parse().ok()) {
        engine.load_preset(i);
    }
    if let Some(m) = arg(&args, "--version") {
        let m = m.to_lowercase();
        if let Some(v) = Version::ALL
            .into_iter()
            .find(|x| x.name().to_lowercase() == m)
        {
            engine.set_version(v);
        }
    }
    if args.iter().any(|a| a == "--stacked") {
        engine.set_stacked(true);
    }
    let sources = Arc::new(Sources::default());
    let output = audio::Output::start(engine.clone(), sources.clone());
    let mut app = App {
        engine,
        sources,
        local: UiLocal::default(),
        status: "Drop a WAV file on the window to loop it.".to_owned(),
        _audio: output,
    };
    if let Some(f) = arg(&args, "--file") {
        app.load(Path::new(&f));
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolPlugs Delay")
            .with_inner_size([DEFAULT_SIZE.0, DEFAULT_SIZE.1 + 34.0])
            .with_min_inner_size([MIN_SIZE.0, MIN_SIZE.1 + 34.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "SolPlugs Delay",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
}

#[cfg(windows)]
mod audio {
    use std::sync::Arc;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use solplugs_delay_app::engine::Engine;

    use crate::source::Sources;

    /// Live input is available on this platform.
    pub const HAS_INPUT: bool = true;

    /// The output stream (and the input stream, when there is an input device).
    pub struct Output {
        _out: cpal::Stream,
        _input: Option<cpal::Stream>,
    }

    fn start_input(device: &cpal::Device, mut tx: rtrb::Producer<f32>) -> Option<cpal::Stream> {
        let config = device.default_input_config().ok()?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return None;
        }
        let ch = usize::from(config.channels()).max(1);
        let stream = device
            .build_input_stream(
                config.into(),
                move |data: &[f32], _| {
                    for f in data.chunks(ch) {
                        let l = f[0];
                        let r = f.get(1).copied().unwrap_or(l);
                        if tx.slots() >= 2 {
                            let _ = tx.push(l);
                            let _ = tx.push(r);
                        }
                    }
                },
                |e| log::warn!("input: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(stream)
    }

    impl Output {
        pub fn start(engine: Arc<Engine>, sources: Arc<Sources>) -> Option<Self> {
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let config = device.default_output_config().ok()?;
            if config.sample_format() != cpal::SampleFormat::F32 {
                log::warn!("output isn't f32; audio is off");
                return None;
            }
            let channels = usize::from(config.channels()).max(1);
            let rate = f64::from(config.sample_rate());
            engine.set_sample_rate(rate as f32);
            let (tx, rx) = rtrb::RingBuffer::new(48_000);
            let input = host
                .default_input_device()
                .and_then(|d| start_input(&d, tx));
            let mut feed = sources.feed(rx);
            let mut proc = engine.processor(rate, 4096);
            let mut l = vec![0.0f32; 8192];
            let mut r = vec![0.0f32; 8192];
            let stream = device
                .build_output_stream(
                    config.into(),
                    move |data: &mut [f32], _| {
                        let frames = data.len() / channels;
                        if l.len() < frames {
                            l.resize(frames, 0.0);
                            r.resize(frames, 0.0);
                        }
                        feed.fill(&mut l[..frames], &mut r[..frames]);
                        proc.process(&mut [&mut l[..frames], &mut r[..frames]], None, None);
                        for (i, f) in data.chunks_mut(channels).enumerate() {
                            f[0] = l[i];
                            if channels > 1 {
                                f[1] = r[i];
                            }
                            for x in f.iter_mut().skip(2) {
                                *x = 0.0;
                            }
                        }
                    },
                    |e| log::warn!("output: {e}"),
                    None,
                )
                .ok()?;
            stream.play().ok()?;
            Some(Self {
                _out: stream,
                _input: input,
            })
        }
    }
}

#[cfg(not(windows))]
mod audio {
    use std::sync::Arc;
    use std::time::Duration;

    use solplugs_delay_app::engine::Engine;

    use crate::source::Sources;

    /// No audio devices on this platform yet.
    pub const HAS_INPUT: bool = false;

    /// No output device here yet: a thread runs the file through the engine in real time
    /// so the meters and waveforms still move.
    pub struct Output;

    impl Output {
        pub fn start(engine: Arc<Engine>, sources: Arc<Sources>) -> Option<Self> {
            std::thread::spawn(move || {
                let (_tx, rx) = rtrb::RingBuffer::new(16);
                let mut feed = sources.feed(rx);
                let mut proc = engine.processor(48_000.0, 480);
                let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
                loop {
                    feed.fill(&mut l, &mut r);
                    proc.process(&mut [&mut l, &mut r], None, None);
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Some(Self)
        }
    }
}
