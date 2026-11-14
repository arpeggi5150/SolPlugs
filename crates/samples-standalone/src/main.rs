//! SolPlugs Samples in its own window. It's a browser, not an effect: the same library and UI
//! as the plugin, with previews on the system's default output.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use solplugs_samples_app::engine::Engine;
use solplugs_samples_app::ui::{UiLocal, show};

struct App {
    engine: Arc<Engine>,
    local: UiLocal,
    _audio: Option<audio::Output>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        show(ui, &self.engine, &mut self.local, Some(frame));
    }
}

fn main() -> eframe::Result {
    let engine = Engine::shared();
    // `--import <folder>` imports on start (handy for scripted tests).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--import")
        && let Some(p) = args.get(i + 1)
    {
        engine.send(solplugs_samples_app::worker::Cmd::Import(vec![p.into()]));
    }
    let output = audio::Output::start(engine.player());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolPlugs Samples")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "SolPlugs Samples",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(App {
                engine,
                local: UiLocal::default(),
                _audio: output,
            }))
        }),
    )
}

#[cfg(windows)]
mod audio {
    use std::sync::Arc;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use solplugs_samples_app::player::Player;

    /// The preview output stream.
    pub struct Output {
        _stream: cpal::Stream,
    }

    impl Output {
        pub fn start(player: Arc<Player>) -> Option<Self> {
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let config = device.default_output_config().ok()?;
            if config.sample_format() != cpal::SampleFormat::F32 {
                log::warn!("output isn't f32; previews are off");
                return None;
            }
            let channels = usize::from(config.channels()).max(1);
            let rate = f64::from(config.sample_rate());
            let mut l = vec![0.0f32; 8192];
            let mut r = vec![0.0f32; 8192];
            let stream = device
                .build_output_stream(
                    &config.into(),
                    move |data: &mut [f32], _| {
                        let frames = data.len() / channels;
                        if l.len() < frames {
                            // Only if the device asks for a block bigger than ever before.
                            l.resize(frames, 0.0);
                            r.resize(frames, 0.0);
                        }
                        l[..frames].fill(0.0);
                        r[..frames].fill(0.0);
                        player.render(&mut l[..frames], &mut r[..frames], rate);
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
                    |e| log::warn!("preview output: {e}"),
                    None,
                )
                .ok()?;
            stream.play().ok()?;
            Some(Self { _stream: stream })
        }
    }
}

#[cfg(not(windows))]
mod audio {
    use std::sync::Arc;

    use solplugs_samples_app::player::Player;

    /// No preview output on this platform yet; a thread advances the player so the playhead
    /// and spectrum still move.
    pub struct Output;

    impl Output {
        pub fn start(player: Arc<Player>) -> Option<Self> {
            std::thread::spawn(move || {
                let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
                loop {
                    player.render(&mut l, &mut r, 48_000.0);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            });
            Some(Self)
        }
    }
}
