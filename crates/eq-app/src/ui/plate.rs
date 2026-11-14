//! Drawing kit for the hardware faceplates (shared design with the compressor's). A faceplate is laid out on a fixed design
//! canvas ([`W`] x [`H`]) and scaled uniformly to fit the space it gets, so it keeps its
//! proportions at any window size. Text never shrinks below [`MIN_TEXT`] pixels.

use std::f32::consts::PI;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::engine::Engine;
use crate::params::{self, Pid};

/// Design canvas width.
pub const W: f32 = 540.0;
/// Design canvas height.
pub const H: f32 = 640.0;
/// Smallest text, pixels.
pub const MIN_TEXT: f32 = 9.5;

/// A scaled drawing surface for one faceplate.
pub struct Plate<'a> {
    /// Where the canvas sits on screen.
    pub rect: Rect,
    /// Design units to pixels.
    pub scale: f32,
    /// Painter clipped to the canvas.
    pub painter: egui::Painter,
    /// The ui (for interaction).
    pub ui: &'a mut Ui,
}

impl<'a> Plate<'a> {
    /// Fits the design canvas into `area`, centred.
    pub fn new(ui: &'a mut Ui, area: Rect) -> Self {
        let scale = (area.width() / W).min(area.height() / H).max(0.1);
        let size = vec2(W * scale, H * scale);
        let rect = Rect::from_center_size(area.center(), size);
        let painter = ui.painter_at(rect.expand(2.0));
        Self {
            rect,
            scale,
            painter,
            ui,
        }
    }

    /// Design point to screen.
    pub fn p(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x, y) * self.scale
    }

    /// Design rect to screen.
    pub fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), vec2(w, h) * self.scale)
    }

    /// Design length to pixels.
    pub fn s(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// Font of design size `size`, never below [`MIN_TEXT`].
    pub fn font(&self, size: f32) -> FontId {
        FontId::proportional((size * self.scale).max(MIN_TEXT))
    }

    /// Text centred at a design point.
    pub fn text(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) {
        self.painter
            .text(self.p(x, y), Align2::CENTER_CENTER, s, self.font(size), c);
    }

    /// Text with an explicit anchor.
    pub fn text_at(&self, x: f32, y: f32, align: Align2, s: &str, font: FontId, c: Color32) {
        self.painter.text(self.p(x, y), align, s, font, c);
    }

    /// Bold-looking text: drawn twice, offset by half a pixel.
    pub fn bold(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) {
        for dx in [0.0, 0.6] {
            self.painter.text(
                self.p(x, y) + vec2(dx, 0.0),
                Align2::CENTER_CENTER,
                s,
                self.font(size),
                c,
            );
        }
    }

    /// Embossed text: a light edge under, a dark one over.
    pub fn emboss(&self, x: f32, y: f32, s: &str, size: f32, c: Color32, light: Color32) {
        let p = self.p(x, y);
        let f = self.font(size);
        self.painter.text(
            p + vec2(0.0, 1.2),
            Align2::CENTER_CENTER,
            s,
            f.clone(),
            light,
        );
        self.painter
            .text(p + vec2(0.6, 0.0), Align2::CENTER_CENTER, s, f.clone(), c);
        self.painter.text(p, Align2::CENTER_CENTER, s, f, c);
    }

    /// The plate itself: a rounded slab with a vertical gradient and fine horizontal
    /// brushing.
    pub fn slab(&self, rect: Rect, top: Color32, bottom: Color32, brush: u8, radius: f32) {
        let cr = CornerRadius::same(self.s(radius).round().clamp(0.0, 255.0) as u8);
        self.painter.rect_filled(rect, cr, bottom);
        // Gradient as stacked bands.
        let bands = 24;
        let inner = rect.shrink(self.s(radius) * 0.3);
        for i in 0..bands {
            let t = i as f32 / bands as f32;
            let c = lerp_colour(top, bottom, t);
            let y0 = inner.top() + inner.height() * t;
            let y1 = inner.top() + inner.height() * (t + 1.0 / bands as f32) + 1.0;
            self.painter.rect_filled(
                Rect::from_x_y_ranges(inner.x_range(), y0..=y1),
                CornerRadius::ZERO,
                c,
            );
        }
        if brush > 0 {
            // Deterministic pseudo-random brushing lines.
            let mut seed = 0x9E37_79B9u32;
            let n = (rect.height() / 2.2) as usize;
            for i in 0..n {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let y = inner.top() + (i as f32 + 0.5) * inner.height() / n as f32;
                let a = (seed >> 24) as u8 % brush;
                let c = if seed & 1 == 0 {
                    Color32::from_white_alpha(a)
                } else {
                    Color32::from_black_alpha(a)
                };
                self.painter.line_segment(
                    [pos2(inner.left(), y), pos2(inner.right(), y)],
                    Stroke::new(1.0, c),
                );
            }
        }
        self.painter.rect_stroke(
            rect,
            cr,
            Stroke::new(self.s(2.0).max(1.0), Color32::from_black_alpha(160)),
            StrokeKind::Inside,
        );
    }

    /// A slotted screw head.
    pub fn screw(&self, x: f32, y: f32, r: f32, metal: Color32) {
        let c = self.p(x, y);
        let r = self.s(r);
        self.painter
            .circle_filled(c + vec2(0.0, 1.0), r, Color32::from_black_alpha(120));
        self.painter.circle_filled(c, r, metal);
        self.painter.circle_filled(
            c - vec2(r * 0.25, r * 0.25),
            r * 0.55,
            Color32::from_white_alpha(40),
        );
        let a = 0.7f32;
        let d = vec2(a.cos(), a.sin()) * r * 0.75;
        self.painter.line_segment(
            [c - d, c + d],
            Stroke::new(r * 0.28, Color32::from_black_alpha(170)),
        );
        self.painter
            .circle_stroke(c, r, Stroke::new(1.0, Color32::from_black_alpha(120)));
    }

    /// Screws in the four corners of a design rect.
    pub fn corner_screws(&self, x: f32, y: f32, w: f32, h: f32, inset: f32, metal: Color32) {
        for (sx, sy) in [
            (x + inset, y + inset),
            (x + w - inset, y + inset),
            (x + inset, y + h - inset),
            (x + w - inset, y + h - inset),
        ] {
            self.screw(sx, sy, 8.0, metal);
        }
    }

    /// A pill-shaped plaque with text.
    pub fn plaque(
        &self,
        rect: Rect,
        text: &str,
        size: f32,
        fill: Color32,
        edge: Color32,
        ink: Color32,
    ) {
        let cr = CornerRadius::same((rect.height() / 2.0).min(255.0) as u8);
        self.painter.rect_filled(rect, cr, fill);
        self.painter.rect_stroke(
            rect,
            cr,
            Stroke::new(self.s(1.5).max(1.0), edge),
            StrokeKind::Inside,
        );
        self.painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            self.font(size),
            ink,
        );
    }
}

/// Mixes two colours.
pub fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// How a knob looks.
#[derive(Clone, Copy, Debug)]
pub struct KnobLook {
    /// Outer skirt / body.
    pub body: Color32,
    /// Top cap.
    pub cap: Color32,
    /// Cap highlight.
    pub shine: Color32,
    /// Pointer line.
    pub pointer: Color32,
    /// Scale ticks and labels.
    pub ink: Color32,
    /// Number of grip ridges (0 = smooth).
    pub ridges: u32,
    /// Draw a tick scale around the knob.
    pub ticks: bool,
}

/// Options for one knob.
#[derive(Clone, Copy, Debug, Default)]
pub struct KnobOpts<'s> {
    /// Turning right moves the value down (1176 attack/release, peak reduction).
    pub reversed: bool,
    /// Label under the knob ("" for none).
    pub label: &'s str,
    /// Small text under the label (scale hint like "SLOW — FAST").
    pub hint: &'s str,
    /// Hide the value readout.
    pub no_value: bool,
    /// Value text override (auto-makeup shows the live gain).
    pub value_override: Option<f32>,
    /// Help shown on hover.
    pub help: &'s str,
}

const START: f32 = PI * 0.75;
const SWEEP: f32 = PI * 1.5;

/// A rotary knob bound to a parameter at design point (x, y), radius r. Drag up/down
/// (Shift for fine), scroll, double-click for the default.
pub fn knob(
    plate: &mut Plate<'_>,
    engine: &Engine,
    id: Pid,
    (x, y, r): (f32, f32, f32),
    look: &KnobLook,
    o: KnobOpts<'_>,
) -> Response {
    let Some(info) = id.info() else {
        return plate.ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
    };
    let c = plate.p(x, y);
    let rr = plate.s(r);
    let hit = Rect::from_center_size(c, egui::Vec2::splat(rr * 2.3));
    let resp = plate
        .ui
        .interact(
            hit,
            plate.ui.scope_id().with(("knob", id.0)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeVertical);
    let value = o
        .value_override
        .map_or_else(|| engine.params().get(id), f64::from);
    let norm = info.normalize(value);
    drag_logic(
        plate.ui,
        engine,
        id,
        &resp,
        norm,
        o.value_override.is_some(),
    );
    let shown = if o.reversed { 1.0 - norm } else { norm } as f32;

    let p = &plate.painter;
    if look.ticks {
        for i in 0..=10 {
            let a = START + SWEEP * i as f32 / 10.0;
            let d = vec2(a.cos(), a.sin());
            let long = i % 5 == 0;
            p.line_segment(
                [
                    c + d * rr * 1.12,
                    c + d * rr * if long { 1.32 } else { 1.24 },
                ],
                Stroke::new(if long { 1.6 } else { 1.0 }, look.ink),
            );
        }
    }
    // Shadow, skirt, ridges, cap, shine.
    p.circle_filled(
        c + vec2(0.0, rr * 0.08),
        rr * 1.02,
        Color32::from_black_alpha(140),
    );
    p.circle_filled(c, rr, look.body);
    if look.ridges > 0 {
        for i in 0..look.ridges {
            let a = i as f32 * std::f32::consts::TAU / look.ridges as f32;
            let d = vec2(a.cos(), a.sin());
            p.line_segment(
                [c + d * rr * 0.80, c + d * rr * 0.98],
                Stroke::new(
                    (rr * 0.05).max(0.8),
                    lerp_colour(look.body, Color32::BLACK, 0.45),
                ),
            );
        }
    }
    p.circle_filled(c, rr * 0.74, look.cap);
    p.circle_filled(c - vec2(rr * 0.2, rr * 0.24), rr * 0.42, look.shine);
    p.circle_filled(c, rr * 0.5, look.cap);
    if resp.hovered() || resp.dragged() {
        p.circle_stroke(
            c,
            rr * 1.04,
            Stroke::new(1.5, Color32::from_white_alpha(70)),
        );
    }
    let a = START + SWEEP * shown;
    let d = vec2(a.cos(), a.sin());
    p.line_segment(
        [c + d * rr * 0.2, c + d * rr * 0.92],
        Stroke::new((rr * 0.09).max(1.5), look.pointer),
    );

    let mut ty = y + r * 1.42 + 8.0;
    if !o.label.is_empty() {
        plate.bold(x, ty, o.label, 13.0, look.ink);
        ty += 15.0;
    }
    if !o.hint.is_empty() {
        plate.text(x, ty, o.hint, 9.5, look.ink.gamma_multiply(0.8));
        ty += 12.0;
    }
    if !o.no_value {
        plate.text(
            x,
            ty,
            &params::format(id, value),
            11.0,
            look.ink.gamma_multiply(0.85),
        );
    }
    let help = format!("{}: {}\n{}", info.name, params::format(id, value), o.help);
    resp.on_hover_text(help)
}

/// Shared drag/scroll/double-click handling for knobs and sliders. `norm` is the current
/// position 0..1. `takes_over` is set when the shown value isn't the parameter (auto-makeup):
/// the first touch hands control back to the parameter.
pub fn drag_logic(ui: &Ui, engine: &Engine, id: Pid, resp: &Response, norm: f64, takes_over: bool) {
    let Some(info) = id.info() else {
        return;
    };
    if (resp.drag_started() || resp.double_clicked()) && takes_over {
        engine.set_auto_makeup(false);
    }
    if resp.drag_started() {
        engine.begin_edit(id);
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let delta = resp.drag_delta();
        let dy = f64::from(-delta.y + delta.x * 0.5) / if fine { 1000.0 } else { 200.0 };
        let base = if takes_over {
            info.normalize(engine.params().get(id))
        } else {
            norm
        };
        engine.edit(id, info.denormalize(base + dy));
    }
    if resp.drag_stopped() {
        engine.end_edit(id);
    }
    if resp.double_clicked() {
        engine.set(id, info.default);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.5 {
            let n = info.normalize(engine.params().get(id)) + f64::from(scroll) / 600.0;
            engine.set(id, info.denormalize(n));
        }
    }
}

/// VU-scale marks: (VU, label).
const VU_MARKS: [(f32, &str); 9] = [
    (-20.0, "20"),
    (-10.0, "10"),
    (-7.0, "7"),
    (-5.0, "5"),
    (-3.0, "3"),
    (-1.0, "1"),
    (0.0, "0"),
    (1.0, "1"),
    (3.0, "3"),
];

/// VU level (0 = reference) to needle position 0..1 on a VU-law scale (-20..+3).
pub fn vu_pos(vu: f32) -> f32 {
    let lin = |v: f32| 10f32.powf(v / 20.0);
    let (lo, hi) = (lin(-20.0), lin(3.0));
    ((lin(vu.clamp(-30.0, 6.0)) - lo) / (hi - lo)).clamp(-0.03, 1.03)
}

/// An analogue VU meter in design rect (x, y, w, h): a lit face, the scale with the red
/// zone above 0 VU, and the needle at `pos` (see [`vu_pos`]).
pub fn vu_meter(
    plate: &Plate<'_>,
    (x, y, w, h): (f32, f32, f32, f32),
    pos: f32,
    face: Color32,
    ink: Color32,
    caption: &str,
) {
    let r = plate.r(x, y, w, h);
    let p = &plate.painter;
    let cr = CornerRadius::same(plate.s(6.0) as u8);
    p.rect_filled(r.expand(plate.s(5.0)), cr, Color32::from_rgb(0x1C, 0x1A, 0x16));
    p.rect_filled(r, cr, face);
    // Lamp glow from below, kept inside the window.
    p.with_clip_rect(r.intersect(p.clip_rect())).circle_filled(
        plate.p(x + w * 0.5, y + h * 1.05),
        plate.s(h * 0.75),
        Color32::from_rgba_unmultiplied(255, 200, 110, 26),
    );
    let c = plate.p(x + w * 0.5, y + h * 0.98);
    let rad = plate.s(h * 0.78).min(plate.s(w * 0.55));
    let (a0, a1) = (PI * 1.22, PI * 1.78);
    let ang = |t: f32| a0 + (a1 - a0) * t;
    let red = Color32::from_rgb(0xC8, 0x2A, 0x1E);
    for i in 0..=48 {
        let t = i as f32 / 48.0;
        let a = ang(t);
        let d = vec2(a.cos(), a.sin());
        let col = if t > vu_pos(0.0) { red } else { ink };
        p.line_segment(
            [c + d * rad * 0.97, c + d * rad * 0.99],
            Stroke::new(plate.s(3.0).max(1.5), col),
        );
    }
    for (vu, label) in VU_MARKS {
        let t = vu_pos(vu);
        let a = ang(t);
        let d = vec2(a.cos(), a.sin());
        let col = if vu > 0.0 { red } else { ink };
        p.line_segment([c + d * rad * 0.88, c + d * rad * 0.99], Stroke::new(1.4, col));
        p.text(
            c + d * rad * 0.78,
            Align2::CENTER_CENTER,
            label,
            plate.font(h * 0.11),
            col,
        );
    }
    plate.text(x + w * 0.5, y + h * 0.62, "VU", h * 0.16, ink);
    if !caption.is_empty() {
        plate.text(x + w * 0.5, y + h * 0.80, caption, h * 0.09, ink.gamma_multiply(0.8));
    }
    let a = ang(pos.clamp(-0.03, 1.03));
    let d = vec2(a.cos(), a.sin());
    p.line_segment(
        [c + d * rad * 0.15, c + d * rad * 1.0],
        Stroke::new(plate.s(1.8).max(1.1), Color32::from_rgb(0x20, 0x1C, 0x18)),
    );
    p.rect_stroke(
        r,
        cr,
        Stroke::new(plate.s(2.0).max(1.0), Color32::from_black_alpha(160)),
        StrokeKind::Inside,
    );
}

/// A clickable lamp-button at design rect. Returns the response.
#[allow(clippy::too_many_arguments)]
pub fn button(
    plate: &mut Plate<'_>,
    salt: (&str, u32),
    rect: Rect,
    label: &str,
    lit: bool,
    face: Color32,
    lit_face: Color32,
    ink: Color32,
) -> Response {
    let resp = plate
        .ui
        .interact(rect, plate.ui.scope_id().with(salt), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let p = &plate.painter;
    let cr = CornerRadius::same(plate.s(5.0) as u8);
    p.rect_filled(
        rect.translate(vec2(0.0, plate.s(2.0))),
        cr,
        Color32::from_black_alpha(150),
    );
    p.rect_filled(rect, cr, if lit { lit_face } else { face });
    if lit {
        p.rect_stroke(
            rect.expand(plate.s(2.0)),
            cr,
            Stroke::new(plate.s(2.0), lit_face.gamma_multiply(0.5)),
            StrokeKind::Outside,
        );
    }
    p.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0, Color32::from_black_alpha(200)),
        StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        plate.font(14.0),
        if lit { Color32::from_gray(20) } else { ink },
    );
    if resp.hovered() {
        p.rect_stroke(
            rect,
            cr,
            Stroke::new(1.0, Color32::from_white_alpha(80)),
            StrokeKind::Outside,
        );
    }
    resp
}
