//! DIFF: where the mix leaves the references' range, shown in colour and said in words.

use egui::{Align2, FontId, RichText, Sense, Stroke, Ui, vec2};
use solplugs_ref_core::bands::BANDS;

use super::UiLocal;
use super::chart::{Chart, means};
use super::theme::Palette;
use super::widgets::legend;
use crate::state::AppState;

pub fn show(ui: &mut Ui, s: &AppState, pal: &Palette, local: &mut UiLocal) {
    let (Some(g), Some(mix)) = (s.group_stats.as_ref(), s.mix.as_ref()) else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(
                    "DIFF needs your mix playing and at least one reference in the group.",
                )
                .size(15.0)
                .color(pal.text_dim),
            );
        });
        return;
    };
    let d = s.diff.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "YOUR MIX AGAINST {} REFERENCE{}",
                g.count,
                if g.count == 1 { "" } else { "S" }
            ))
            .strong()
            .size(12.0),
        );
        legend(ui, pal.group, "Reference range", pal);
        legend(ui, pal.within, "Inside", pal);
        legend(ui, pal.above, "Above", pal);
        legend(ui, pal.below, "Below", pal);
    });
    let avail = ui.available_size();
    let side = 300.0f32.min(avail.x * 0.35);
    ui.horizontal_top(|ui| {
        let chart_h = (avail.y - 150.0).max(180.0);
        let (rect, resp) =
            ui.allocate_exact_size(vec2(avail.x - side - 12.0, chart_h), Sense::hover());
        let p = ui.painter_at(rect);
        let tilt = s.library.settings.display_tilt_db_per_oct;
        let gm = means(&g.tonal);
        let chart = Chart::fit(rect.shrink(2.0), tilt, &[&mix.tonal.mean_db, &gm], 24.0);
        chart.grid(&p, pal);
        if let Some((lo, hi)) = local.highlight {
            chart.highlight(&p, lo, hi, pal.text.gamma_multiply(0.06));
        }
        chart.span_band(&p, &g.tonal, pal.group.gamma_multiply(0.3));
        chart.line(&p, &gm, Stroke::new(1.0, pal.group.gamma_multiply(0.8)));
        // Deviation fill: from the nearest edge of the range to the mix.
        let edge: Vec<f64> = (0..BANDS)
            .map(|b| match g.tonal.get(b).copied().flatten() {
                Some(sp) => mix.tonal.mean_db[b].clamp(sp.lo, sp.hi),
                None => f64::NAN,
            })
            .collect();
        let bands = &d.bands;
        chart.fill_between(&p, &edge, &mix.tonal.mean_db, |b| {
            bands.get(b).and_then(|x| {
                (x.outside != 0.0).then(|| pal.verdict(x.outside).gamma_multiply(0.45))
            })
        });
        chart.line_coloured(&p, &mix.tonal.mean_db, 2.5, |b| {
            bands.get(b).map_or(pal.mix, |x| pal.verdict(x.outside))
        });
        if let Some(pos) = resp.hover_pos() {
            let hz = chart.hz_at(pos.x);
            if let Some(b) = solplugs_ref_core::bands::band_of(hz) {
                let x = bands.get(b).copied().unwrap_or_default();
                let text = if x.outside == 0.0 {
                    format!(
                        "{hz:.0} Hz: inside the range ({:+.1} dB from the mean)",
                        x.to_mean
                    )
                } else {
                    format!(
                        "{hz:.0} Hz: {:.1} dB {} the range",
                        x.outside.abs(),
                        if x.outside > 0.0 { "above" } else { "below" }
                    )
                };
                p.line_segment(
                    [
                        egui::pos2(pos.x, rect.top()),
                        egui::pos2(pos.x, rect.bottom()),
                    ],
                    Stroke::new(1.0, pal.text_dim),
                );
                p.text(
                    egui::pos2(pos.x + 6.0, rect.top() + 22.0),
                    Align2::LEFT_TOP,
                    text,
                    FontId::proportional(12.0),
                    pal.text,
                );
            }
        }
        ui.vertical(|ui| {
            ui.set_width(side);
            ui.label(RichText::new("WHAT'S DIFFERENT").strong().size(12.0));
            if d.issues.is_empty() {
                ui.label(
                    RichText::new("Tonal balance sits inside the references' range.")
                        .color(pal.within),
                );
            }
            local.highlight = None;
            egui::ScrollArea::vertical()
                .id_salt("issues")
                .max_height(chart_h - 20.0)
                .show(ui, |ui| {
                    for issue in &d.issues {
                        let resp = super::theme::card(pal)
                            .show(ui, |ui| {
                                ui.set_width(side - 24.0);
                                ui.horizontal(|ui| {
                                    let (r, _) =
                                        ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                    ui.painter().circle_filled(
                                        r.center(),
                                        4.0,
                                        pal.verdict(issue.worst_db),
                                    );
                                    ui.label(RichText::new(&issue.text).color(pal.text));
                                });
                            })
                            .response;
                        if resp.hovered() {
                            local.highlight = Some((issue.lo_hz, issue.hi_hz));
                        }
                    }
                });
        });
    });
    ui.add_space(6.0);
    ui.label(RichText::new("METRICS").strong().size(12.0));
    egui::Grid::new("metrics")
        .num_columns(4)
        .spacing(vec2(24.0, 4.0))
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("").size(11.0));
            ui.label(RichText::new("Mix").color(pal.mix).size(11.0));
            ui.label(RichText::new("References").color(pal.group).size(11.0));
            ui.label(RichText::new("").size(11.0));
            ui.end_row();
            for m in &d.metrics {
                ui.label(&m.name);
                ui.label(
                    RichText::new(format!("{:.2} {}", m.value, m.unit))
                        .monospace()
                        .color(pal.verdict(m.outside)),
                );
                ui.label(
                    RichText::new(format!("{:.2} to {:.2}", m.range.lo, m.range.hi))
                        .monospace()
                        .color(pal.text_dim),
                );
                let verdict = if m.outside == 0.0 {
                    "in range".to_string()
                } else {
                    format!("{:+.2} outside", m.outside)
                };
                ui.label(RichText::new(verdict).color(pal.verdict(m.outside)));
                ui.end_row();
            }
        });
}
