//! PARTS: the sub, kick and snare outlined in the references, with the mix's version on top.

use egui::{RichText, Sense, Stroke, Ui, vec2};
use solplugs_ref_core::parts::PartKind;

use super::UiLocal;
use super::chart::{Chart, means};
use super::theme::{Palette, card};
use crate::state::AppState;

pub fn show(ui: &mut Ui, s: &AppState, pal: &Palette, local: &mut UiLocal) {
    let Some(g) = s.group_stats.as_ref() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("PARTS needs at least one reference in the group.")
                    .size(15.0)
                    .color(pal.text_dim),
            );
        });
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("ELEMENTS").strong().size(12.0));
        for k in PartKind::ALL {
            let mut on = !local.hidden_parts.contains(&k);
            let found = g
                .parts
                .iter()
                .find(|p| p.kind == k)
                .map_or(0, |p| p.found_in);
            let label =
                RichText::new(format!("{} ({found}/{})", k.label(), g.count)).color(pal.part(k));
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    local.hidden_parts.retain(|h| *h != k);
                } else {
                    local.hidden_parts.push(k);
                }
            }
        }
        ui.label(
            RichText::new("Filled: references' range · solid: their average · dashed: your mix")
                .size(11.0)
                .color(pal.text_dim),
        );
    });
    let avail = ui.available_size();
    let (rect, _) =
        ui.allocate_exact_size(vec2(avail.x, (avail.y - 170.0).max(180.0)), Sense::hover());
    let p = ui.painter_at(rect);
    let tilt = s.library.settings.display_tilt_db_per_oct;
    let context = means(&g.tonal);
    let shown: Vec<_> = g
        .parts
        .iter()
        .filter(|ps| ps.found_in > 0 && !local.hidden_parts.contains(&ps.kind))
        .collect();
    let mut curves: Vec<Vec<f64>> = vec![context.clone()];
    curves.extend(shown.iter().map(|ps| means(&ps.outline)));
    let refs: Vec<&[f64]> = curves.iter().map(Vec::as_slice).collect();
    let chart = Chart::fit(rect.shrink(2.0), tilt, &refs, 30.0);
    chart.grid(&p, pal);
    chart.line(
        &p,
        &context,
        Stroke::new(1.0, pal.group.gamma_multiply(0.35)),
    );
    for ps in &shown {
        let c = pal.part(ps.kind);
        chart.span_band(&p, &ps.outline, c.gamma_multiply(0.28));
        chart.line(&p, &means(&ps.outline), Stroke::new(2.0, c));
        if let Some(mine) = s
            .mix
            .as_ref()
            .and_then(|m| m.parts.get(ps.kind))
            .filter(|m| m.found)
        {
            chart.dashed(
                &p,
                &mine.outline_db,
                Stroke::new(2.0, c.gamma_multiply(0.9)),
            );
        }
        if let Some(peak) = ps.peak_hz {
            let x = chart.x_hz(peak.mean);
            p.text(
                egui::pos2(x, rect.top() + 20.0),
                egui::Align2::CENTER_TOP,
                ps.kind.label(),
                egui::FontId::proportional(12.0),
                c,
            );
        }
    }
    ui.add_space(6.0);
    let diffs = s.diff.as_ref().map(|d| d.parts.clone()).unwrap_or_default();
    ui.horizontal_top(|ui| {
        let w = (ui.available_width() - 24.0) / 3.0;
        for k in PartKind::ALL {
            let ps = g.parts.iter().find(|p| p.kind == k);
            let mine = s
                .mix
                .as_ref()
                .and_then(|m| m.parts.get(k))
                .filter(|m| m.found);
            card(pal).show(ui, |ui| {
                ui.set_width(w - 20.0);
                ui.label(RichText::new(k.label()).strong().color(pal.part(k)));
                let Some(ps) = ps.filter(|p| p.found_in > 0) else {
                    ui.label(RichText::new("Not found in the references").color(pal.text_dim));
                    return;
                };
                let span = |o: Option<solplugs_ref_core::group::Span>, unit: &str, dec: usize| {
                    o.map_or("—".into(), |s| {
                        format!("{:.dec$}–{:.dec$} {unit}", s.lo, s.hi)
                    })
                };
                let row = |ui: &mut Ui, name: &str, refs: String, mix: Option<String>| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(11.0).color(pal.text_dim));
                        ui.label(RichText::new(refs).monospace().size(11.0).color(pal.group));
                        if let Some(m) = mix {
                            ui.label(RichText::new(m).monospace().size(11.0).color(pal.mix));
                        }
                    });
                };
                row(
                    ui,
                    "Centre",
                    span(ps.peak_hz, "Hz", 0),
                    mine.map(|m| format!("{:.0} Hz", m.peak_hz)),
                );
                row(
                    ui,
                    "Level",
                    span(ps.level_db, "dB", 1),
                    mine.map(|m| format!("{:.1} dB", m.level_db)),
                );
                if k != PartKind::Sub {
                    row(
                        ui,
                        "Decay",
                        span(ps.decay_ms, "ms", 0),
                        mine.map(|m| format!("{:.0} ms", m.decay_ms)),
                    );
                    row(
                        ui,
                        "Punch",
                        span(ps.punch_db, "dB", 1),
                        mine.map(|m| format!("{:.1} dB", m.punch_db)),
                    );
                }
                if let Some(d) = diffs.iter().find(|d| d.kind == k) {
                    if d.text.is_empty() {
                        ui.label(RichText::new("Matches the references").color(pal.within));
                    }
                    for t in &d.text {
                        ui.label(RichText::new(t).color(pal.text));
                    }
                }
            });
        }
    });
}
