//! The flat digital side view of the hardware styles: the analyzer (before and after
//! spectra with the style's EQ curve, in overlay, side-by-side or stacked view), the BEFORE
//! and AFTER waveforms, the post meter (average peak before and after) and MATCH. Laid out
//! by hand so it squeezes cleanly when the window is small.

use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use solplugs_eq_core::style::hardware_bands;
use solplugs_eq_core::{BandSpec, Style};

use super::analyzer::{Analyzer, Which};
use super::graph::{self, Axes};
use super::theme::Palette;
use super::{SpecView, UiLocal, db_text, match_row, waveform};
use crate::engine::Engine;

/// Below this height the view drops its caption and shrinks the rows.
const COMPACT: f32 = 560.0;

/// Cuts rows off the top of a rect.
pub(crate) struct Cursor {
    pub rect: Rect,
    pub y: f32,
    pub gap: f32,
}

impl Cursor {
    pub fn take(&mut self, h: f32) -> Rect {
        let r = Rect::from_min_size(pos2(self.rect.left(), self.y), vec2(self.rect.width(), h));
        self.y += h + self.gap;
        r
    }
}

/// The EQ curve of a hardware style as its knobs stand (dB at a frequency).
pub fn hardware_curve(engine: &Engine, style: Style) -> impl Fn(f32) -> f32 {
    let s = engine.params().style_settings(style);
    let bands: Vec<BandSpec> = hardware_bands(style, &s.knobs).to_vec();
    let fs = f64::from(engine.sample_rate()) * solplugs_eq_core::hw::OS as f64;
    move |f: f32| {
        bands
            .iter()
            .map(|b| b.magnitude_db(f64::from(f), fs))
            .sum::<f64>() as f32
    }
}

/// Draws the side view into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let painter = ui.painter_at(area.expand(1.0));
    painter.rect_filled(area, CornerRadius::same(10), pal.panel);
    let inner = area.shrink(12.0);
    let compact = inner.height() < COMPACT;
    let (head, post, matchb, caption, gap) = if compact {
        (24.0, 40.0, 34.0, 0.0, 5.0)
    } else {
        (30.0, 54.0, 46.0, 16.0, 8.0)
    };
    let fixed = head + post + matchb + caption + gap * 6.0;
    let graphs = (inner.height() - fixed).max(120.0);
    let mut cur = Cursor {
        rect: inner,
        y: inner.top(),
        gap,
    };
    header(ui, &painter, cur.take(head), engine, local, pal);
    let curve = hardware_curve(engine, engine.style());
    spectrometer(
        &painter,
        cur.take(graphs * 0.58),
        &local.analyzer,
        local.spec_view,
        &curve,
        pal,
        true,
    );
    waveform(&painter, cur.take(graphs * 0.21), &local.history, false, pal);
    waveform(&painter, cur.take(graphs * 0.21), &local.history, true, pal);
    post_meter(&painter, cur.take(post), engine, pal);
    match_row(ui, &painter, cur.take(matchb), engine, pal);
    if caption > 0.0 {
        let r = cur.take(caption);
        let style = engine.style();
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            format!(
                "Latency {:.2} ms  ·  4x oversampled  ·  MATCH sets Gain Out so AFTER is as loud as BEFORE",
                engine.latency_ms(style)
            ),
            FontId::proportional(11.0),
            pal.dim,
        );
    }
}

fn header(ui: &mut Ui, p: &egui::Painter, r: Rect, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let font = FontId::proportional((r.height() * 0.58).max(13.0));
    let t = p.text(
        r.left_center(),
        Align2::LEFT_CENTER,
        "DIGITAL MATCH · ",
        font.clone(),
        pal.dim,
    );
    let t2 = p.text(
        t.right_center(),
        Align2::LEFT_CENTER,
        "ANALYZER",
        font.clone(),
        pal.text,
    );
    let name = engine.style().name().to_uppercase();
    let pill_font = FontId::proportional((r.height() * 0.48).max(11.0));
    let g = p.layout_no_wrap(format!("Style: {name}"), pill_font, pal.text);
    let toggle_w = 200.0f32.min(r.width() * 0.42);
    let pill = Rect::from_min_size(
        pos2(t2.right() + 10.0, r.center().y - g.size().y / 2.0 - 3.0),
        g.size() + vec2(16.0, 6.0),
    );
    if pill.right() < r.right() - toggle_w - 6.0 {
        p.rect_stroke(
            pill,
            CornerRadius::same(10),
            Stroke::new(1.5, pal.before),
            StrokeKind::Inside,
        );
        p.galley(pill.min + vec2(8.0, 3.0), g, pal.text);
    }
    let tr = Rect::from_min_max(pos2(r.right() - toggle_w, r.top()), r.max);
    view_toggle(ui, p, tr, &mut local.spec_view, pal);
}

/// The Overlay / Side by Side / Stacked toggle.
pub(crate) fn view_toggle(ui: &mut Ui, p: &egui::Painter, r: Rect, view: &mut SpecView, pal: &Palette) {
    let opts = [
        (SpecView::Overlay, "Overlay"),
        (SpecView::SideBySide, "Side by Side"),
        (SpecView::Stacked, "Stacked"),
    ];
    let gap = 4.0;
    let w = (r.width() - gap * 2.0) / 3.0;
    let font = FontId::proportional((r.height() * 0.42).clamp(9.5, 13.0));
    for (i, (v, label)) in opts.into_iter().enumerate() {
        let b = Rect::from_min_size(pos2(r.left() + i as f32 * (w + gap), r.top()), vec2(w, r.height()));
        let resp = ui
            .interact(b, ui.scope_id().with(("specview", i)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("How the before and after spectrums share the spectrometer: on one graph, side by side, or stacked above/below.");
        let sel = *view == v;
        p.rect_filled(
            b,
            CornerRadius::same(6),
            if sel {
                pal.before
            } else if resp.hovered() {
                pal.grid
            } else {
                pal.raised
            },
        );
        p.text(b.center(), Align2::CENTER_CENTER, label, font.clone(), pal.text);
        if resp.clicked() {
            *view = v;
        }
    }
}

/// A spectrometer: before (blue) and after (orange) spectra, laid out per `view`, with the
/// EQ curve over them.
pub(crate) fn spectrometer(
    p: &egui::Painter,
    r: Rect,
    an: &Analyzer,
    view: SpecView,
    curve: &dyn Fn(f32) -> f32,
    pal: &Palette,
    detailed: bool,
) {
    p.rect_filled(r, CornerRadius::same(8), pal.card);
    let label_h = if detailed { 16.0 } else { 13.0 };
    let left = if detailed { 30.0 } else { 6.0 };
    let plot = Rect::from_min_max(
        pos2(r.left() + left, r.top() + if detailed { 22.0 } else { 6.0 }),
        pos2(r.right() - 8.0, r.bottom() - label_h - 2.0),
    );
    if detailed {
        p.text(
            r.left_top() + vec2(10.0, 5.0),
            Align2::LEFT_TOP,
            "SPECTRUM ANALYZER",
            FontId::proportional(12.0),
            pal.dim,
        );
        legend(p, pos2(r.right() - 10.0, r.top() + 5.0), pal);
    }
    let panes: Vec<(Rect, Option<Which>)> = match view {
        SpecView::Overlay => vec![(plot, None)],
        SpecView::SideBySide => {
            let w = (plot.width() - 8.0) / 2.0;
            vec![
                (Rect::from_min_size(plot.min, vec2(w, plot.height())), Some(Which::Before)),
                (
                    Rect::from_min_size(pos2(plot.left() + w + 8.0, plot.top()), vec2(w, plot.height())),
                    Some(Which::After),
                ),
            ]
        }
        SpecView::Stacked => {
            let h = (plot.height() - label_h - 4.0) / 2.0;
            vec![
                (Rect::from_min_size(plot.min, vec2(plot.width(), h)), Some(Which::Before)),
                (
                    Rect::from_min_size(
                        pos2(plot.left(), plot.top() + h + label_h + 4.0),
                        vec2(plot.width(), h),
                    ),
                    Some(Which::After),
                ),
            ]
        }
    };
    for (rect, which) in panes {
        let a = Axes {
            rect,
            db_range: 18.0,
            spec: (-84.0, 0.0),
        };
        let pp = p.with_clip_rect(rect.expand2(vec2(1.0, label_h + 2.0)));
        graph::grid(&pp, &a, pal, true, detailed && rect.left() < plot.left() + 1.0);
        let clip = p.with_clip_rect(rect);
        if which != Some(Which::After) {
            graph::spectrum(
                &clip,
                &a,
                an,
                Which::Before,
                pal.before,
                Some(pal.before.gamma_multiply(0.16)),
            );
        }
        if which != Some(Which::Before) {
            graph::spectrum(
                &clip,
                &a,
                an,
                Which::After,
                pal.after,
                (which == Some(Which::After)).then(|| pal.after.gamma_multiply(0.16)),
            );
        }
        graph::curve(&clip, &a, Stroke::new(2.0, pal.text.gamma_multiply(0.85)), curve);
        if let Some(w) = which {
            let (t, c) = match w {
                Which::Before => ("BEFORE", pal.before),
                Which::After => ("AFTER", pal.after),
            };
            p.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, t, FontId::proportional(10.5), c);
        }
    }
}

fn legend(p: &egui::Painter, right_top: egui::Pos2, pal: &Palette) {
    let font = FontId::proportional(11.5);
    let a = p.text(right_top, Align2::RIGHT_TOP, "AFTER", font.clone(), pal.after);
    p.circle_filled(pos2(a.left() - 6.0, a.center().y), 3.5, pal.after);
    let b = p.text(pos2(a.left() - 16.0, right_top.y), Align2::RIGHT_TOP, "BEFORE", font.clone(), pal.before);
    p.circle_filled(pos2(b.left() - 6.0, b.center().y), 3.5, pal.before);
    let c = p.text(pos2(b.left() - 16.0, right_top.y), Align2::RIGHT_TOP, "EQ", font, pal.text);
    p.line_segment(
        [pos2(c.left() - 16.0, c.center().y), pos2(c.left() - 4.0, c.center().y)],
        Stroke::new(2.0, pal.text),
    );
}

/// Bar position of a level, 0..1 over -60..0 dBFS.
pub(crate) fn bar_pos(db: f32) -> f32 {
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// The post meter: average peak before and after, as bars with readouts.
fn post_meter(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let m = engine.meters();
    let h = (r.height() - 6.0) / 2.0;
    for (i, (label, v, c)) in [
        ("BEFORE", m.avg_before(), pal.before),
        ("AFTER", m.avg_after(), pal.after),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::from_min_size(pos2(r.left(), r.top() + i as f32 * (h + 6.0)), vec2(r.width(), h));
        p.rect_filled(row, CornerRadius::same(5), pal.card);
        let font = FontId::proportional((h * 0.55).clamp(10.0, 13.0));
        p.text(row.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, label, font.clone(), c);
        let val = p.text(
            row.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            db_text(v),
            FontId::monospace((h * 0.55).clamp(10.0, 13.0)),
            pal.text,
        );
        let bar = Rect::from_min_max(
            pos2(row.left() + 70.0, row.top() + h * 0.3),
            pos2(val.left() - 10.0, row.bottom() - h * 0.3),
        );
        p.rect_filled(bar, CornerRadius::same(3), pal.bg);
        let fill = Rect::from_min_max(bar.min, pos2(bar.left() + bar.width() * bar_pos(v), bar.bottom()));
        p.rect_filled(fill, CornerRadius::same(3), c);
    }
    let _ = Color32::TRANSPARENT;
}
