//! The manager's UI, drawn with egui. [`show`] draws everything into a `Ui`; the plugin and
//! standalone window call it from their own windows, Solara from a panel.

mod detail;
mod grid;
mod pack_view;
mod sidebar;
pub mod textures;
pub mod theme;
mod topbar;
pub mod widgets;

use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Ui};
use raw_window_handle::HasWindowHandle;
use solplugs_samples_core::filter::{Query, Versions};
use solplugs_samples_core::model::Id;
use solplugs_samples_core::spectrum::Spectrum;

use crate::engine::{Engine, Shared};
use crate::worker::Cmd;
use theme::Palette;

/// Which part of the library the grid shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    All,
    Favorites,
    Raws,
    Edited,
    Pack(Id),
}

/// Things that block (file dialogs, OS drags) and so run after the library lock is released.
pub(crate) enum Action {
    PickImport,
    PickCover(Id),
    PickArt(Id),
    PickExport(Id),
    DragOut {
        paths: Vec<PathBuf>,
        image: Option<PathBuf>,
    },
}

/// UI-only state.
pub struct UiLocal {
    styled: bool,
    pub view: View,
    pub query: Query,
    /// The sample shown in the detail panel.
    pub selected: Option<Id>,
    /// Time selection on the detail waveform, seconds.
    pub selection: Option<(f64, f64)>,
    sel_anchor: Option<f64>,
    /// Fade length for the fade tools, ms.
    pub fade_ms: f32,
    new_tag: String,
    new_genre: String,
    new_pack: String,
    path_input: String,
    textures: textures::Textures,
    spectrum: Spectrum,
    spectrum_for: Option<String>,
    /// Cached filter result: (revision, view, query) -> sample indices.
    results: Option<(u64, View, Query, Vec<usize>)>,
    /// Detail waveform columns cache: (id, width) -> (min, max).
    detail_cols: Option<(String, usize, Vec<f32>, Vec<f32>)>,
    confirm_remove: Option<Id>,
    confirm_pack_delete: Option<Id>,
    hovered: Option<Id>,
}

impl Default for UiLocal {
    fn default() -> Self {
        Self {
            styled: false,
            view: View::All,
            query: Query::default(),
            selected: None,
            selection: None,
            sel_anchor: None,
            fade_ms: 10.0,
            new_tag: String::new(),
            new_genre: String::new(),
            new_pack: String::new(),
            path_input: String::new(),
            textures: textures::Textures::default(),
            spectrum: Spectrum::new(),
            spectrum_for: None,
            results: None,
            detail_cols: None,
            confirm_remove: None,
            confirm_pack_delete: None,
            hovered: None,
        }
    }
}

impl UiLocal {
    /// Selects a sample (clears the time selection when it changes).
    pub fn select(&mut self, id: Option<Id>) {
        if self.selected != id {
            self.selection = None;
            self.sel_anchor = None;
        }
        self.selected = id;
    }

    /// Indices of the samples the grid shows.
    fn results(&mut self, s: &Shared) -> Vec<usize> {
        let mut q = self.query.clone();
        match &self.view {
            View::All => {}
            View::Favorites => q.favorites = true,
            View::Raws => q.versions = Versions::Raws,
            View::Edited => q.versions = Versions::Edited,
            View::Pack(p) => q.pack = Some(p.clone()),
        }
        if let Some((rev, v, cq, r)) = &self.results
            && *rev == s.revision
            && *v == self.view
            && *cq == self.query
        {
            return r.clone();
        }
        let r = q.run(&s.lib);
        self.results = Some((s.revision, self.view.clone(), self.query.clone(), r.clone()));
        r
    }
}

/// Native folder picker (Windows/macOS).
fn pick_folder(title: &str) -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = title;
        None
    }
}

/// Native image picker.
fn pick_image() -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .set_title("Choose an image")
            .pick_file()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// True where native file pickers exist (elsewhere the UI offers a path field).
#[must_use]
pub fn has_dialogs() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// Draws the whole manager. `window` is the host window, used to start OS drags of samples.
pub fn show(
    ui: &mut Ui,
    engine: &Engine,
    local: &mut UiLocal,
    window: Option<&dyn HasWindowHandle>,
) {
    if !local.styled {
        theme::apply(ui.ctx());
        local.styled = true;
    }
    let pal = Palette::default();
    local.textures.begin_frame();
    let mut actions = Vec::new();
    {
        let Some(mut guard) = engine.lock() else {
            ui.label("SolPlugs Samples stopped unexpectedly. Close and reopen the window.");
            return;
        };
        let s: &mut Shared = &mut guard;
        let playing = engine.player().is_playing();
        if playing || s.import.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        egui::Panel::top("smp-top")
            .frame(theme::bar(&pal))
            .show(ui, |ui| topbar::show(ui, s, local, &pal, engine));
        egui::Panel::bottom("smp-status")
            .frame(theme::bar(&pal))
            .show(ui, |ui| status_bar(ui, s, &pal, engine));
        egui::Panel::left("smp-side")
            .frame(theme::sidebar(&pal))
            .resizable(true)
            .default_size(230.0)
            .min_size(190.0)
            .max_size(340.0)
            .show(ui, |ui| {
                sidebar::show(ui, s, local, &pal, engine, &mut actions)
            });
        if local
            .selected
            .as_ref()
            .is_some_and(|id| s.lib.sample(id).is_none())
        {
            local.select(None);
        }
        if local.selected.is_some() {
            egui::Panel::right("smp-detail")
                .frame(theme::detail(&pal))
                .resizable(true)
                .default_size(420.0)
                .min_size(360.0)
                .max_size(640.0)
                .show(ui, |ui| {
                    detail::show(ui, s, local, &pal, engine, &mut actions)
                });
        }
        egui::CentralPanel::default()
            .frame(theme::body(&pal))
            .show(ui, |ui| {
                if let View::Pack(id) = local.view.clone() {
                    if s.lib.pack(&id).is_some() {
                        pack_view::show(ui, s, local, &pal, engine, &id, &mut actions);
                    } else {
                        local.view = View::All;
                    }
                }
                grid::show(ui, s, local, &pal, engine, &mut actions);
            });
        keyboard(ui, s, local, engine);
        file_drop(ui, &pal, engine);
    }
    run_actions(actions, engine, window);
}

fn run_actions(actions: Vec<Action>, engine: &Engine, window: Option<&dyn HasWindowHandle>) {
    for a in actions {
        match a {
            Action::PickImport => {
                if let Some(p) = pick_folder("Import a folder of samples") {
                    engine.send(Cmd::Import(vec![p]));
                }
            }
            Action::PickCover(pack) => {
                if let Some(image) = pick_image() {
                    engine.send(Cmd::SetCover { pack, image });
                }
            }
            Action::PickArt(sample) => {
                if let Some(image) = pick_image() {
                    engine.send(Cmd::SetArt { sample, image });
                }
            }
            Action::PickExport(pack) => {
                if let Some(dest) = pick_folder("Export the pack into…") {
                    engine.send(Cmd::Export { pack, dest });
                }
            }
            Action::DragOut { paths, image } => {
                let Some(w) = window else { continue };
                if let Err(e) = crate::dragout::start(w, &paths, image)
                    && let Some(mut s) = engine.lock()
                {
                    s.note(format!("Couldn't drag the sample: {e}"), true);
                }
            }
        }
    }
}

/// Space plays/stops the selected sample; left/right step through the grid and play.
fn keyboard(ui: &mut Ui, s: &mut Shared, local: &mut UiLocal, engine: &Engine) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let (space, left, right, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if esc {
        engine.stop();
        local.selection = None;
    }
    if space && let Some(smp) = local.selected.as_ref().and_then(|id| s.lib.sample(id)) {
        let player = engine.player();
        if player.is_playing() {
            engine.stop();
        } else {
            let (from, to) = local.selection.map_or((0.0, None), |(a, b)| (a, Some(b)));
            engine.preview(&smp.id, &smp.path, &smp.fx, from, to);
        }
    }
    if left || right {
        let r = local.results(s);
        if r.is_empty() {
            return;
        }
        let cur = local
            .selected
            .as_ref()
            .and_then(|id| r.iter().position(|&i| s.lib.samples[i].id == *id));
        let next = match cur {
            None => 0,
            Some(c) if right => (c + 1).min(r.len() - 1),
            Some(c) => c.saturating_sub(1),
        };
        let smp = &s.lib.samples[r[next]];
        local.select(Some(smp.id.clone()));
        engine.preview(&smp.id, &smp.path, &smp.fx, 0.0, None);
    }
}

/// Files or folders dropped on the window are imported.
fn file_drop(ui: &mut Ui, pal: &Palette, engine: &Engine) {
    let (hovering, dropped) = ui.ctx().input(|i| {
        (
            !i.raw.hovered_files.is_empty(),
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect::<Vec<_>>(),
        )
    });
    if hovering {
        let rect = ui.ctx().content_rect();
        let p = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("smp-drop"),
        ));
        p.rect_filled(rect, 0.0, Color32::from_black_alpha(170));
        p.rect_stroke(
            rect.shrink(12.0),
            16.0,
            egui::Stroke::new(2.0, pal.accent),
            egui::StrokeKind::Inside,
        );
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop to import",
            FontId::proportional(28.0),
            pal.text,
        );
    }
    if !dropped.is_empty() {
        engine.send(Cmd::Import(dropped));
    }
}

fn status_bar(ui: &mut Ui, s: &mut Shared, pal: &Palette, engine: &Engine) {
    ui.horizontal(|ui| {
        if let Some(p) = &s.import {
            let frac = if p.total > 0 {
                p.done as f32 / p.total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(220.0)
                    .text(format!("Importing {}/{}", p.done, p.total)),
            );
            ui.label(egui::RichText::new(widgets::ellipsize(&p.current, 50)).color(pal.text_dim));
            if ui.button("Stop").clicked() {
                engine.send(Cmd::CancelImport);
            }
        } else if let Some(e) = &s.load_error {
            ui.label(
                egui::RichText::new(format!(
                    "The library file couldn't be read, so changes aren't saved: {e}"
                ))
                .color(pal.danger),
            );
        } else if let Some(n) = s.notes.back().filter(|n| n.at.elapsed().as_secs() < 12) {
            ui.label(egui::RichText::new(&n.text).color(if n.error {
                pal.danger
            } else {
                pal.text_dim
            }));
        } else {
            let raws = s
                .lib
                .samples
                .iter()
                .filter(|x| !x.version.is_edited())
                .count();
            ui.label(
                egui::RichText::new(format!(
                    "{} samples · {} raw · {} edited · {} packs",
                    s.lib.samples.len(),
                    raws,
                    s.lib.samples.len() - raws,
                    s.lib.packs.len()
                ))
                .color(pal.text_faint),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (txt, c) = if solplugs_songsense::available() {
                ("BPM/key: Song Sense", pal.ok)
            } else {
                (
                    "BPM/key detection off (Song Sense not in this build)",
                    pal.text_faint,
                )
            };
            ui.label(egui::RichText::new(txt).size(11.0).color(c));
        });
    });
}
