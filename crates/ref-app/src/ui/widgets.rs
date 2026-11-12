//! Small custom widgets: the A/B switch, tabs, metric tiles with range bars.

use egui::{
    Align2, Color32, CornerRadius, FontId, Response, RichText, Sense, Stroke, StrokeKind, Ui, vec2,
};
use solplugs_ref_core::group::Span;

use super::theme::Palette;

/// Shortens text to `max` characters with an ellipsis.
#[must_use]
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

/// "m:ss".
#[must_use]
pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The big A/B switch. Returns the new state when clicked.
pub fn ab_switch(ui: &mut Ui, listening_ref: bool, ref_ready: bool, pal: &Palette) -> Option<bool> {
    let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(15), pal.panel_hi);
    let half = rect.width() / 2.0;
    let knob = if listening_ref {
        egui::Rect::from_min_size(rect.min + vec2(half, 0.0), vec2(half, rect.height()))
    } else {
        egui::Rect::from_min_size(rect.min, vec2(half, rect.height()))
    };
    let colour = if listening_ref {
        pal.reference
    } else {
        pal.mix
    };
    p.rect_filled(knob.shrink(2.0), CornerRadius::same(13), colour);
    let font = FontId::proportional(13.0);
    let (a_col, b_col) = if listening_ref {
        (pal.text_dim, pal.bg)
    } else {
        (pal.bg, if ref_ready { pal.text_dim } else { pal.grid })
    };
    p.text(
        rect.left_center() + vec2(half / 2.0, 0.0),
        Align2::CENTER_CENTER,
        "A  MIX",
        font.clone(),
        a_col,
    );
    p.text(
        rect.left_center() + vec2(half * 1.5, 0.0),
        Align2::CENTER_CENTER,
        "B  REF",
        font,
        b_col,
    );
    let resp = resp.on_hover_text(if ref_ready {
        "Flip between your mix (A) and the reference (B)"
    } else {
        "Pick a reference first"
    });
    if resp.clicked() {
        if !listening_ref && !ref_ready {
            return None;
        }
        return Some(!listening_ref);
    }
    None
}

/// A tab button.
pub fn tab(ui: &mut Ui, label: &str, selected: bool, pal: &Palette) -> Response {
    let text = RichText::new(label).size(12.0).strong().color(if selected {
        pal.text
    } else {
        pal.text_dim
    });
    let resp = ui.add(egui::Button::new(text).frame(false));
    if selected {
        let r = resp.rect;
        ui.painter().line_segment(
            [
                r.left_bottom() + vec2(0.0, 2.0),
                r.right_bottom() + vec2(0.0, 2.0),
            ],
            Stroke::new(2.0, pal.mix),
        );
    }
    resp
}

/// What a metric tile shows.
pub struct Tile<'a> {
    pub name: &'a str,
    pub value: Option<f64>,
    pub unit: &'a str,
    pub range: Option<Span>,
    /// Slack around the range before the value counts as outside.
    pub tolerance: f64,
    pub decimals: usize,
}

/// A metric tile: name, the mix's value, and a bar showing the references' range with the
/// mix's position on it.
pub fn metric_tile(ui: &mut Ui, pal: &Palette, t: Tile<'_>) {
    let Tile {
        name,
        value,
        unit,
        range,
        tolerance,
        decimals,
    } = t;
    let (rect, _) = ui.allocate_exact_size(vec2(150.0, 72.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(8), pal.panel);
    p.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, pal.grid),
        StrokeKind::Inside,
    );
    p.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(11.0),
        pal.text_dim,
    );
    let outside = match (value, range) {
        (Some(v), Some(r)) => Some(r.outside(v, tolerance)),
        _ => None,
    };
    let colour = outside.map_or(pal.text, |o| pal.verdict(o));
    let shown = value.map_or("—".to_string(), |v| format!("{v:.decimals$}"));
    p.text(
        rect.left_top() + vec2(10.0, 24.0),
        Align2::LEFT_TOP,
        &shown,
        FontId::monospace(20.0),
        colour,
    );
    p.text(
        rect.right_top() + vec2(-10.0, 30.0),
        Align2::RIGHT_TOP,
        unit,
        FontId::proportional(11.0),
        pal.text_dim,
    );
    // Range bar.
    let bar = egui::Rect::from_min_max(
        rect.left_bottom() + vec2(10.0, -16.0),
        rect.right_bottom() + vec2(-10.0, -10.0),
    );
    p.rect_filled(bar, CornerRadius::same(3), pal.panel_hi);
    if let Some(r) = range {
        let pad = ((r.hi - r.lo).abs() * 0.75).max(tolerance * 4.0).max(1e-6);
        let (lo, hi) = (
            r.lo.min(value.unwrap_or(r.lo)) - pad,
            r.hi.max(value.unwrap_or(r.hi)) + pad,
        );
        let x = |v: f64| bar.left() + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 * bar.width();
        let band = egui::Rect::from_min_max(
            egui::pos2(x(r.lo - tolerance), bar.top()),
            egui::pos2(
                x(r.hi + tolerance).max(x(r.lo - tolerance) + 2.0),
                bar.bottom(),
            ),
        );
        p.rect_filled(band, CornerRadius::same(3), pal.group.gamma_multiply(0.45));
        if let Some(v) = value {
            let xv = x(v);
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(xv, bar.center().y), vec2(3.0, 12.0)),
                CornerRadius::same(1),
                colour,
            );
        }
    } else {
        p.text(
            bar.center(),
            Align2::CENTER_CENTER,
            "no references",
            FontId::proportional(9.0),
            pal.text_dim,
        );
    }
}

/// A coloured dot with a label.
pub fn legend(ui: &mut Ui, colour: Color32, label: &str, pal: &Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.0, colour);
    ui.label(RichText::new(label).size(11.0).color(pal.text_dim));
}
