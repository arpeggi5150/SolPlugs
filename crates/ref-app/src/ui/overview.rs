//! OVERVIEW: the meters, and the mix's tonal balance over the references'.

use egui::{RichText, Sense, Stroke, Ui, vec2};

use super::chart::{Chart, means};
use super::theme::Palette;
use super::widgets::{Tile, legend, metric_tile};
use crate::state::AppState;

pub fn show(ui: &mut Ui, s: &AppState, pal: &Palette) {
    let g = s.group_stats.as_ref();
    let mix = s.mix.as_ref();
    let r = &s.readings;
    ui.horizontal_wrapped(|ui| {
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Short-term",
                value: r.short_term_lufs,
                unit: "LUFS",
                range: g.and_then(|g| g.integrated_lufs),
                tolerance: 1.0,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Integrated",
                value: mix.and_then(|m| m.integrated_lufs),
                unit: "LUFS",
                range: g.and_then(|g| g.integrated_lufs),
                tolerance: 0.5,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "True peak",
                value: mix.map(|m| m.true_peak_db),
                unit: "dBTP",
                range: g.and_then(|g| g.true_peak_db),
                tolerance: 0.2,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "RMS",
                value: mix.map(|m| m.rms_db),
                unit: "dBFS",
                range: g.and_then(|g| g.rms_db),
                tolerance: 0.5,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Loudness range",
                value: mix.and_then(|m| m.loudness_range),
                unit: "LU",
                range: g.and_then(|g| g.loudness_range),
                tolerance: 0.5,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Peak to loudness",
                value: mix.and_then(|m| m.plr_db),
                unit: "dB",
                range: g.and_then(|g| g.plr_db),
                tolerance: 0.5,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Crest",
                value: mix.map(|m| m.crest_db),
                unit: "dB",
                range: g.and_then(|g| g.crest_db),
                tolerance: 0.5,
                decimals: 1,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Width",
                value: r.width.or(mix.and_then(|m| m.width)).map(|w| w * 100.0),
                unit: "%",
                range: g
                    .and_then(|g| g.width)
                    .map(|w| solplugs_ref_core::group::Span {
                        lo: w.lo * 100.0,
                        mean: w.mean * 100.0,
                        hi: w.hi * 100.0,
                    }),
                tolerance: 3.0,
                decimals: 0,
            },
        );
        metric_tile(
            ui,
            pal,
            Tile {
                name: "Correlation",
                value: r.correlation,
                unit: "",
                range: g.and_then(|g| g.correlation),
                tolerance: 0.03,
                decimals: 2,
            },
        );
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("TONAL BALANCE")
                .strong()
                .size(12.0)
                .color(pal.text),
        );
        legend(ui, pal.mix, "Mix (since reset)", pal);
        legend(ui, pal.mix.gamma_multiply(0.5), "Mix (last 3 s)", pal);
        legend(ui, pal.reference, "Playing reference", pal);
        legend(ui, pal.group, "Reference group range", pal);
    });
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(vec2(avail.x, avail.y.max(160.0)), Sense::hover());
    let p = ui.painter_at(rect);
    let tilt = s.library.settings.display_tilt_db_per_oct;
    let active = s
        .active
        .as_ref()
        .and_then(|id| s.find(id))
        .map(|r| r.profile.tonal.mean_db);
    let group_mean = g.map(|g| means(&g.tonal));
    let mix_curve = mix.map(|m| m.tonal.mean_db.clone());
    let mut curves: Vec<&[f64]> = vec![&r.spectrum_db];
    for c in [&mix_curve, &active, &group_mean].into_iter().flatten() {
        curves.push(c);
    }
    let chart = Chart::fit(rect.shrink(2.0), tilt, &curves, 30.0);
    chart.grid(&p, pal);
    if let Some(g) = g {
        chart.span_band(&p, &g.tonal, pal.group.gamma_multiply(0.25));
    }
    chart.line(
        &p,
        &r.spectrum_db,
        Stroke::new(1.0, pal.mix.gamma_multiply(0.45)),
    );
    if let Some(a) = &active {
        chart.line(&p, a, Stroke::new(1.5, pal.reference));
    }
    if let Some(m) = &mix_curve {
        chart.line(&p, m, Stroke::new(2.5, pal.mix));
    }
    if s.mix.is_none() && s.group.is_empty() {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Play your mix, then add references in LIBRARY, SEARCH or SOLROUTE",
            egui::FontId::proportional(14.0),
            pal.text_dim,
        );
    }
}
