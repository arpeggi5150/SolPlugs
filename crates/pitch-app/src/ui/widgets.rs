//! Small controls bound to engine parameters: knobs, toggles, pickers.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, pos2, vec2,
};

use super::theme::Palette;
use crate::engine::Engine;
use crate::params::{self, P};

/// A rotary knob for a continuous parameter: drag up/down (Shift for fine), double-click
/// for the default, hover shows what it does.
pub fn knob(ui: &mut Ui, engine: &Engine, p: P, pal: &Palette, help: &str) -> Response {
    let info = p.info();
    let size = vec2(76.0, 92.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let resp = resp
        .on_hover_cursor(CursorIcon::ResizeVertical)
        .on_hover_text(help);
    let value = engine.params().get(p);
    let norm = info.normalize(value);
    if resp.drag_started() {
        engine.begin_edit(p);
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let dy = -resp.drag_delta().y as f64 / if fine { 900.0 } else { 180.0 };
        engine.edit(p, info.denormalize(norm + dy));
    }
    if resp.drag_stopped() {
        engine.end_edit(p);
    }
    if resp.double_clicked() {
        engine.set(p, info.default);
    }
    let p_ = ui.painter_at(rect);
    let c = pos2(rect.center().x, rect.top() + 34.0);
    let r = 24.0;
    let start = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let arc = |from: f32, to: f32| -> Vec<egui::Pos2> {
        let n = 40;
        (0..=n)
            .map(|k| {
                let a = from + (to - from) * k as f32 / n as f32;
                pos2(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect()
    };
    p_.add(Shape::line(
        arc(start, start + sweep),
        Stroke::new(4.0, pal.panel_hi),
    ));
    // Bipolar parameters fill from the centre.
    let zero = if info.min < 0.0 { 0.5 } else { 0.0 };
    let (a0, a1) = (start + sweep * zero as f32, start + sweep * norm as f32);
    if (a1 - a0).abs() > 0.001 {
        p_.add(Shape::line(
            arc(a0.min(a1), a0.max(a1)),
            Stroke::new(4.0, pal.output),
        ));
    }
    let a = start + sweep * norm as f32;
    p_.circle_filled(
        c,
        r - 7.0,
        if resp.hovered() || resp.dragged() {
            pal.grid
        } else {
            pal.panel_hi
        },
    );
    p_.line_segment(
        [
            pos2(c.x + (r - 16.0) * a.cos(), c.y + (r - 16.0) * a.sin()),
            pos2(c.x + (r - 6.0) * a.cos(), c.y + (r - 6.0) * a.sin()),
        ],
        Stroke::new(2.0, pal.text),
    );
    p_.text(
        pos2(rect.center().x, rect.top() + 66.0),
        Align2::CENTER_CENTER,
        info.short_label(),
        FontId::proportional(12.0),
        pal.text_dim,
    );
    p_.text(
        pos2(rect.center().x, rect.top() + 82.0),
        Align2::CENTER_CENTER,
        params::format(p, value),
        FontId::proportional(12.0),
        pal.text,
    );
    resp
}

/// Label used under knobs.
trait ShortLabel {
    fn short_label(&self) -> &'static str;
}

impl ShortLabel for params::Info {
    fn short_label(&self) -> &'static str {
        match self.name {
            "Retune Speed" => "Retune",
            "Formant Shift" => "Formant",
            n => n,
        }
    }
}

/// A two-state pill bound to a boolean parameter.
pub fn toggle(
    ui: &mut Ui,
    engine: &Engine,
    p: P,
    label: &str,
    on_colour: Color32,
    pal: &Palette,
    help: &str,
) -> Response {
    let on = engine.params().on(p);
    let text = egui::RichText::new(label).color(if on { pal.bg } else { pal.text_dim });
    let b = egui::Button::new(text)
        .fill(if on { on_colour } else { pal.panel_hi })
        .corner_radius(CornerRadius::same(12));
    let resp = ui.add(b).on_hover_text(help);
    if resp.clicked() {
        engine.set(p, if on { 0.0 } else { 1.0 });
    }
    resp
}

/// A drop-down for a stepped parameter.
pub fn picker(ui: &mut Ui, engine: &Engine, p: P, width: f32) {
    let info = p.info();
    let cur = engine.params().get(p);
    egui::ComboBox::from_id_salt(("param", p.id()))
        .width(width)
        .selected_text(params::format(p, cur))
        .show_ui(ui, |ui| {
            for i in 0..info.steps {
                let v = info.min + f64::from(i);
                if ui
                    .selectable_label((cur - v).abs() < 0.5, params::format(p, v))
                    .clicked()
                {
                    engine.set(p, v);
                }
            }
        });
}

/// A horizontal meter centred on zero: `value` in -1..1.
pub fn centre_meter(ui: &mut Ui, value: Option<f32>, width: f32, colour: Color32, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(4), pal.panel_hi);
    let mid = rect.center().x;
    p.line_segment(
        [pos2(mid, rect.top() - 2.0), pos2(mid, rect.bottom() + 2.0)],
        Stroke::new(1.0, pal.text_dim),
    );
    if let Some(v) = value {
        let v = v.clamp(-1.0, 1.0);
        let x = mid + v * rect.width() / 2.0;
        let bar = egui::Rect::from_min_max(
            pos2(mid.min(x), rect.top() + 2.0),
            pos2(mid.max(x), rect.bottom() - 2.0),
        );
        p.rect_filled(bar, CornerRadius::same(2), colour);
        p.circle_filled(pos2(x, rect.center().y), 4.0, colour);
    }
    p.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, pal.grid),
        StrokeKind::Inside,
    );
}
