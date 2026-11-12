//! The analyzer's UI, drawn with egui. [`show`] draws everything into a `Ui`; the plugin calls
//! it from its own window, Solara from a panel.

mod chart;
mod diff_view;
mod library_view;
mod overview;
mod parts_view;
mod route_view;
mod search_view;
pub mod theme;
mod widgets;

use egui::{Align, Layout, RichText, Ui};

use crate::engine::Engine;
use crate::state::{AppState, Command, View};
use theme::Palette;

/// UI-only state that doesn't belong in the engine (text being typed, toggles).
#[derive(Default)]
pub struct UiLocal {
    pub group_name: String,
    pub path_input: String,
    pub search_input: String,
    pub capture_title: String,
    pub rename: Option<(String, String, String)>,
    pub hidden_parts: Vec<solplugs_ref_core::parts::PartKind>,
    pub highlight: Option<(f64, f64)>,
    pub styled: bool,
}

/// Draws the whole analyzer.
pub fn show(ui: &mut Ui, engine: &Engine, local: &mut UiLocal) {
    if !local.styled {
        theme::apply(ui.ctx());
        local.styled = true;
    }
    let pal = Palette::default();
    // Repaint for the meters while the window is up.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
    let Some(state) = engine.with_state(|s| s.clone()) else {
        ui.label("The analyzer stopped unexpectedly. Close and reopen the plugin window.");
        return;
    };
    let mut cmds: Vec<Command> = Vec::new();

    egui::Panel::top("ref-top")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| top_bar(ui, engine, &state, &pal, &mut cmds));
    egui::Panel::bottom("ref-status")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| status_bar(ui, &state, &pal, &mut cmds));
    egui::CentralPanel::default()
        .frame(theme::body_frame(&pal))
        .show(ui, |ui| match state.view {
            View::Overview => overview::show(ui, &state, &pal),
            View::Diff => diff_view::show(ui, &state, &pal, local),
            View::Parts => parts_view::show(ui, &state, &pal, local),
            View::Library => library_view::show(ui, &state, &pal, local, &mut cmds),
            View::Search => search_view::show(ui, &state, &pal, local, &mut cmds),
            View::Route => route_view::show(ui, &state, &pal, local, &mut cmds),
        });

    for c in cmds {
        engine.send(c);
    }
}

fn top_bar(ui: &mut Ui, engine: &Engine, s: &AppState, pal: &Palette, cmds: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("SOLPLUGS")
                .strong()
                .color(pal.text_dim)
                .size(12.0),
        );
        ui.label(
            RichText::new("Reference")
                .strong()
                .color(pal.text)
                .size(16.0),
        );
        ui.add_space(16.0);
        let listening_ref = engine.bridge().listening_to_reference();
        if let Some(b) = widgets::ab_switch(ui, listening_ref, s.active_ready, pal) {
            engine.set_listen_reference(b);
        }
        ui.add_space(8.0);
        // Reference picker (the group's members, then everything else).
        let all = s.all_refs();
        let current = s
            .active
            .as_ref()
            .and_then(|id| all.iter().find(|r| &r.id == id))
            .map_or("Pick a reference", |r| r.title.as_str());
        egui::ComboBox::from_id_salt("active-ref")
            .width(220.0)
            .selected_text(widgets::ellipsize(current, 30))
            .show_ui(ui, |ui| {
                for r in all
                    .iter()
                    .filter(|r| s.group.contains(&r.id))
                    .chain(all.iter().filter(|r| !s.group.contains(&r.id)))
                {
                    let label = if s.group.contains(&r.id) {
                        format!("● {}", r.title)
                    } else {
                        r.title.clone()
                    };
                    if ui
                        .selectable_label(s.active.as_ref() == Some(&r.id), label)
                        .clicked()
                    {
                        cmds.push(Command::SetActive(r.id.clone()));
                    }
                }
            });
        let mut lm = s.level_match;
        if ui
            .checkbox(&mut lm, "Level match")
            .on_hover_text("Plays the reference at your mix's integrated loudness, so A/B is fair.")
            .changed()
        {
            cmds.push(Command::SetLevelMatch(lm));
        }
        if s.level_match && s.active.is_some() {
            ui.label(
                RichText::new(format!("{:+.1} dB", s.ref_gain_db))
                    .monospace()
                    .color(pal.reference),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button("Reset")
                .on_hover_text("Start measuring the mix again (new section or a new bounce).")
                .clicked()
            {
                cmds.push(Command::ResetMix);
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (v, label) in [
            (View::Overview, "OVERVIEW"),
            (View::Diff, "DIFF"),
            (View::Parts, "PARTS"),
            (View::Library, "LIBRARY"),
            (View::Search, "SEARCH"),
            (View::Route, "SOLROUTE"),
        ] {
            if widgets::tab(ui, label, s.view == v, pal).clicked() {
                cmds.push(Command::SetView(v));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{} in group", s.group.len())).color(pal.text_dim));
        });
    });
}

fn status_bar(ui: &mut Ui, s: &AppState, pal: &Palette, cmds: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        if !s.audio_running {
            ui.label(
                RichText::new("Waiting for audio: press play in your DAW").color(pal.text_dim),
            );
        } else {
            ui.label(
                RichText::new(format!(
                    "Mix measured: {}",
                    widgets::duration(s.mix_seconds)
                ))
                .color(pal.text_dim),
            );
        }
        for b in &s.busy {
            ui.spinner();
            ui.label(RichText::new(b).color(pal.text_dim));
        }
        if let Some(n) = s.notices.last() {
            ui.separator();
            ui.label(RichText::new(&n.text).color(if n.error { pal.above } else { pal.text }));
            if ui.small_button("✕").clicked() {
                cmds.push(Command::DismissNotices);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs every view headless for a few frames, with and without data, to catch panics.
    #[test]
    fn every_view_draws_without_panicking() {
        let dir = std::env::temp_dir().join(format!("solplugs-ui-test-{}", std::process::id()));
        let engine = Engine::with_library(dir.join("library.json"));
        let ctx = egui::Context::default();
        let mut local = UiLocal::default();
        for view in [
            View::Overview,
            View::Diff,
            View::Parts,
            View::Library,
            View::Search,
            View::Route,
        ] {
            engine.with_state(|s| s.view = view);
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    show(ui, &engine, &mut local)
                });
                out.textures_delta.clear();
            }
        }
        // With a mix and a group.
        let sr = 48_000.0;
        let sig: Vec<f32> = (0..sr as usize * 6)
            .map(|i| (i as f32 * 0.05).sin() * 0.3 + (i as f32 * 0.31).sin() * 0.1)
            .collect();
        let p = solplugs_ref_core::profile::analyze(&sig, &sig, sr);
        engine.with_state(|s| {
            let id = s
                .library
                .add(
                    "Ref",
                    "A",
                    solplugs_ref_io::library::Origin::Capture,
                    p.clone(),
                )
                .unwrap_or_default();
            s.group = vec![id.clone()];
            s.active = Some(id);
            s.mix = Some(p.clone());
            let g = solplugs_ref_core::group::group_stats(&[&p]);
            s.diff = g
                .as_ref()
                .map(|g| solplugs_ref_core::diff::diff(&p, g, Default::default()));
            s.group_stats = g;
            s.library.settings.online_search = true;
        });
        for view in [
            View::Overview,
            View::Diff,
            View::Parts,
            View::Library,
            View::Search,
            View::Route,
        ] {
            engine.with_state(|s| s.view = view);
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    show(ui, &engine, &mut local)
                });
                out.textures_delta.clear();
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
