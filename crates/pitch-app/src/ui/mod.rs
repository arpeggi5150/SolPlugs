//! The editor, drawn with egui. [`show`] draws everything into a `Ui`; the plugin calls it
//! from its own window, Solara from a panel.

mod auto_view;
mod graph_view;
pub mod theme;
mod widgets;

use std::collections::VecDeque;

use egui::{RichText, Ui};

use crate::engine::{Engine, TrailPoint};
use crate::params::P;
use theme::Palette;

/// UI-only state (view position, drags, the pitch trail).
#[derive(Default)]
pub struct UiLocal {
    styled: bool,
    trail: VecDeque<TrailPoint>,
    trail_centre: f32,
    graph: graph_view::GraphView,
}

/// Draws the whole editor.
pub fn show(ui: &mut Ui, engine: &Engine, local: &mut UiLocal) {
    if !local.styled {
        theme::apply(ui.ctx());
        local.styled = true;
    }
    let pal = Palette::default();
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
    engine.drain_trail(|p| {
        local.trail.push_back(p);
    });
    while local.trail.len() > auto_view::TRAIL_LEN {
        local.trail.pop_front();
    }

    egui::Panel::top("pitch-top")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| top_bar(ui, engine, &pal));
    egui::CentralPanel::default()
        .frame(theme::body_frame(&pal))
        .show(ui, |ui| {
            if engine.params().on(P::Mode) {
                graph_view::show(ui, engine, &mut local.graph, &pal);
            } else {
                auto_view::show(ui, engine, local, &pal);
            }
        });
}

fn top_bar(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("SOLPLUGS").strong().color(pal.text_dim).size(12.0));
        ui.label(RichText::new("Pitch").strong().color(pal.text).size(16.0));
        ui.add_space(12.0);
        let graph = engine.params().on(P::Mode);
        for (label, v, help) in [
            ("Automatic", 0.0, "Real-time correction to a key and scale (Auto-Tune style)."),
            ("Graph", 1.0, "Capture a take, then edit notes by hand (Melodyne style)."),
        ] {
            let sel = graph == (v > 0.5);
            if ui
                .selectable_label(sel, RichText::new(label).size(14.0))
                .on_hover_text(help)
                .clicked()
                && !sel
            {
                engine.set(P::Mode, v);
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Key").color(pal.text_dim));
        widgets::picker(ui, engine, P::Key, 48.0);
        widgets::picker(ui, engine, P::Scale, 130.0);
        ui.add_space(8.0);
        widgets::picker(ui, engine, P::Range, 96.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::toggle(
                ui,
                engine,
                P::Bypass,
                "Bypass",
                pal.record,
                pal,
                "True bypass: the input passes through untouched (delayed by the latency so the track stays in time).",
            );
            let lat = engine.latency();
            let sr = engine.sample_rate();
            ui.label(
                RichText::new(format!("Latency {:.1} ms", lat as f32 * 1000.0 / sr))
                    .color(pal.text_dim),
            )
            .on_hover_text(format!(
                "{lat} samples at {sr:.0} Hz. The lowest note the range covers sets it: High voice is shortest, Low voice longest. Hosts compensate automatically."
            ));
        });
    });
}
