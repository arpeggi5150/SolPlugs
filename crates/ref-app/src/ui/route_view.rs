//! SOLROUTE: capture whatever plays on this machine (a streaming app, a browser) as a reference.

use egui::{RichText, Sense, Stroke, Ui, vec2};

use super::UiLocal;
use super::chart::Chart;
use super::theme::{Palette, card};
use super::widgets::{Tile, duration, metric_tile};
use crate::state::{AppState, Command};

pub fn show(
    ui: &mut Ui,
    s: &AppState,
    pal: &Palette,
    local: &mut UiLocal,
    cmds: &mut Vec<Command>,
) {
    let r = &s.route;
    ui.horizontal(|ui| {
        ui.label(RichText::new("SOLROUTE FEED-IN").strong().size(12.0));
        let current = r
            .selected
            .as_ref()
            .and_then(|id| r.sources.iter().find(|(i, _, _)| i == id))
            .map_or_else(|| "Default output".to_string(), |(_, n, _)| n.clone());
        egui::ComboBox::from_id_salt("route-src")
            .width(260.0)
            .selected_text(current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(r.selected.is_none(), "Default output")
                    .clicked()
                {
                    cmds.push(Command::RouteSelect(None));
                }
                for (id, name, is_default) in &r.sources {
                    let label = if *is_default {
                        format!("{name} (default)")
                    } else {
                        name.clone()
                    };
                    if ui
                        .selectable_label(r.selected.as_ref() == Some(id), label)
                        .clicked()
                    {
                        cmds.push(Command::RouteSelect(Some(id.clone())));
                    }
                }
            });
        if ui.button("⟳").on_hover_text("Refresh devices").clicked() {
            cmds.push(Command::RouteRefresh);
        }
        if r.running.is_some() {
            if ui.button("Stop").clicked() {
                cmds.push(Command::RouteStop);
            }
            ui.label(
                RichText::new(format!("● Capturing · {}", duration(r.seconds))).color(pal.above),
            );
        } else if ui.button("Start capture").clicked() {
            cmds.push(Command::RouteStart);
        }
    });
    if let Some(e) = &r.error {
        ui.label(RichText::new(e).color(pal.above));
    }
    card(pal).show(ui, |ui| {
        ui.label(RichText::new("Play the song anywhere on this computer (a streaming app, a browser tab), start the capture, and let it run. Then make it a reference: it joins the group, plays on B loudness-matched, and its analysis can be saved. The captured audio stays in memory only.").color(pal.text_dim));
        ui.label(RichText::new("Tip: stop your DAW while capturing, or the capture hears your mix too.").size(11.0).color(pal.text_dim));
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut local.capture_title)
                .hint_text("Name this reference")
                .desired_width(260.0),
        );
        let enough = r.running.is_some() && r.seconds >= 3.0;
        if ui
            .add_enabled(enough, egui::Button::new("Use capture as reference"))
            .on_disabled_hover_text("Capture at least 3 seconds first")
            .clicked()
        {
            cmds.push(Command::RouteUse {
                title: local.capture_title.clone(),
            });
        }
        ui.label(
            RichText::new("Keeps the last 3 minutes")
                .size(11.0)
                .color(pal.text_dim),
        );
    });
    if r.running.is_some() {
        ui.horizontal_wrapped(|ui| {
            metric_tile(
                ui,
                pal,
                Tile {
                    name: "Short-term",
                    value: r.readings.short_term_lufs,
                    unit: "LUFS",
                    range: None,
                    tolerance: 0.0,
                    decimals: 1,
                },
            );
            metric_tile(
                ui,
                pal,
                Tile {
                    name: "Peak",
                    value: Some(r.readings.peak_db),
                    unit: "dBFS",
                    range: None,
                    tolerance: 0.0,
                    decimals: 1,
                },
            );
            metric_tile(
                ui,
                pal,
                Tile {
                    name: "Width",
                    value: r.readings.width.map(|w| w * 100.0),
                    unit: "%",
                    range: None,
                    tolerance: 0.0,
                    decimals: 0,
                },
            );
        });
        let avail = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(vec2(avail.x, avail.y.max(120.0)), Sense::hover());
        let p = ui.painter_at(rect);
        let chart = Chart::fit(
            rect.shrink(2.0),
            s.library.settings.display_tilt_db_per_oct,
            &[&r.readings.spectrum_db],
            30.0,
        );
        chart.grid(&p, pal);
        chart.line(&p, &r.readings.spectrum_db, Stroke::new(2.0, pal.reference));
    }
}
