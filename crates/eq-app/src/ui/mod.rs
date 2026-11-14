//! The editor, drawn with egui. [`show`] draws everything into a `Ui`; the plugin calls it
//! from its own window, Solara and the standalone app from a panel.
//!
//! Layout, same as the compressor: a header (☀ SolPlugs EQ, style tabs, presets, A/B,
//! bypass, latency) with the style's plain-words info line under it, and a footer with
//! Gain In, Gain Out, Mix, Wet, Dry, auto-makeup, stereo link and colour. The body is
//! two views side by side:
//! - hardware styles: the faceplate on the left, the digital side view on the right
//!   (analyzer with before/after spectra and the EQ curve, BEFORE/AFTER waveforms, post
//!   meter, MATCH);
//! - Clean: the big EQ graph (about two thirds of the width) with the band controls under
//!   it, and a narrow side column (spectrometer, waveforms, BEFORE/AFTER meters, MATCH).

pub mod analyzer;
mod clean;
mod digital;
mod faceplates;
pub mod graph;
mod plate;
pub mod theme;

use std::collections::VecDeque;

use egui::{Color32, CornerRadius, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use solplugs_eq_core::Style;

use crate::engine::{Column, Engine};
use crate::params::{self, C, G, Pid};
use crate::presets::PRESETS;
use theme::Palette;

/// Default editor size, logical pixels.
pub const DEFAULT_SIZE: (f32, f32) = (1240.0, 800.0);
/// Smallest editor size that stays usable, logical pixels.
pub const MIN_SIZE: (f32, f32) = (900.0, 600.0);

/// About five seconds of display columns.
const HISTORY: usize = 1000;

/// How the before/after spectra share the spectrometer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecView {
    /// Both on one graph.
    #[default]
    Overlay,
    /// Before left, after right.
    SideBySide,
    /// Before above, after below.
    Stacked,
}

/// UI-only state: waveform history, the analyzer, needle ballistics, Clean's selection.
pub struct UiLocal {
    styled: bool,
    history: VecDeque<Column>,
    analyzer: analyzer::Analyzer,
    vu: [f32; 2],
    vu_primed: bool,
    /// Selected Clean band.
    selected: Option<usize>,
    /// Show each band's own curve under the combined curve.
    show_bands: bool,
    /// Clean graph range, ±dB.
    db_range: f32,
    /// Spectrometer layout.
    spec_view: SpecView,
}

impl Default for UiLocal {
    fn default() -> Self {
        Self {
            styled: false,
            history: VecDeque::new(),
            analyzer: analyzer::Analyzer::default(),
            vu: [0.0; 2],
            vu_primed: false,
            selected: None,
            show_bands: true,
            db_range: 12.0,
            spec_view: SpecView::Overlay,
        }
    }
}

/// The plain-words line for each style: what it is, what it's styled after, what it was
/// used on.
pub fn info_line(style: Style) -> &'static str {
    match style {
        Style::Pultec => {
            "Pultec: smooth, wide, musical tube EQ with broad shelves. Styled after the Pulse Techniques EQP-1A (1950s): the classic vocal and bass EQ, famous for boosting and cutting the same low frequency at once. Best on vocals, bass and the mix bus."
        }
        Style::Api => {
            "API: punchy, forward and tight, with proportional Q. Styled after the API 550A/B (1970s): the rock console EQ, where the more you boost the narrower the band gets so drums and guitars cut through. Best on drums, guitars and vocals that need to sit forward."
        }
        Style::Ssl => {
            "SSL: clean, precise, surgical console parametric. Styled after the SSL G-Series EQ (1980s): the mix-bus EQ on countless records, clean enough to carve a whole mix and never muddy. Best on the mix bus, drums and anything that needs a clean carve."
        }
        Style::Neve => {
            "Neve: warm, big and round, Class A colour. Styled after the Neve 1073 (1970s): the warm vocal EQ, big low end and sweet top, the sound of a thousand classic records. Best on vocals, bass and anything that should sound expensive."
        }
        Style::Clean => {
            "Clean: transparent modern digital, the super-clean one. Styled after no hardware: the bar is FabFilter Pro-Q and REAPER's ReEQ, the DAW-era precise EQ for when the sound must stay exactly as it is. Best for surgical cuts, notches and anything that must stay exact."
        }
        Style::Massive => {
            "Massive: huge, smooth mastering tube EQ with four bands of warm passive curves. Styled after the Manley Massive Passive (1990s): the mastering EQ, big and expensive-sounding, the other great tube EQ next to Pultec. Best on the mix bus, mastering and vocals that need size."
        }
        Style::Maag => {
            "Maag: airy, open and sparkling, the Air Band. Styled after the Maag EQ4 (2000s): its Air Band shelf reaches up to 40 kHz and adds air without harshness, the top-end trick on countless vocals. Best on vocals, the mix bus and anything that needs air."
        }
        Style::Curve => {
            "Curve: bold, musical Abbey Road colour with wide curves. Styled after the Chandler Limited Curve Bender (EMI / Abbey Road console EQ): the Beatles desk sound, bold curves and instant vibe. Best on drums, the mix bus and anything that needs vibe."
        }
    }
}

/// Draws the whole editor.
pub fn show(ui: &mut Ui, engine: &Engine, local: &mut UiLocal) {
    if !local.styled {
        theme::apply(ui.ctx());
        local.styled = true;
    }
    let pal = Palette::default();
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
    engine.drain_columns(|c| local.history.push_back(c));
    while local.history.len() > HISTORY {
        local.history.pop_front();
    }
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    local.analyzer.update(engine, dt);
    local.update_vu(engine, dt);

    egui::Panel::top("eq-top")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| {
            top_bar(ui, engine, &pal);
            ui.add(
                egui::Label::new(
                    RichText::new(info_line(engine.style()))
                        .color(pal.dim)
                        .size(12.5),
                )
                .wrap(),
            );
        });
    egui::Panel::bottom("eq-footer")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| footer(ui, engine, &pal));
    egui::CentralPanel::default()
        .frame(theme::body_frame(&pal))
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let style = engine.style();
            if style == Style::Clean {
                clean::show(ui, area, engine, local, &pal);
            } else {
                let (left, right) = split(area);
                faceplates::show(ui, left, engine, local.vu);
                digital::show(ui, right, engine, local, &pal);
            }
        });
}

/// Splits the body for a hardware style: the faceplate gets the width its proportions
/// want (40-50%), the side view the rest.
fn split(area: Rect) -> (Rect, Rect) {
    let want = area.height() * plate::W / plate::H;
    let w = want.min(area.width() * 0.5).max(area.width() * 0.4);
    let left = Rect::from_min_size(area.min, vec2(w, area.height()));
    let right = Rect::from_min_max(pos2(left.right() + 10.0, area.top()), area.max);
    (left, right)
}

impl UiLocal {
    /// VU needles: output level with VU ballistics (about 300 ms). 0 VU = -18 dBFS.
    fn update_vu(&mut self, engine: &Engine, dt: f32) {
        let (l, r) = engine.meters().output_lr();
        let k = 1.0 - (-dt / 0.065).exp();
        for (n, db) in self.vu.iter_mut().zip([l, r]) {
            let t = plate::vu_pos(db + 18.0);
            *n = if self.vu_primed { *n + (t - *n) * k } else { t };
        }
        self.vu_primed = true;
    }
}

/// The ☀ mark, drawn so it never depends on an emoji font.
fn sun_mark(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    p.circle_filled(c, 5.0, colour);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * 7.5, c + d * 10.5], Stroke::new(1.8, colour));
    }
}

fn top_bar(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    ui.horizontal_wrapped(|ui| {
        sun_mark(ui, pal.sun);
        ui.label(RichText::new("SolPlugs").strong().color(pal.sun).size(17.0));
        ui.label(RichText::new("EQ").strong().color(pal.text).size(15.0));
        ui.add_space(8.0);
        style_tabs(ui, engine, pal);
        ui.add_space(8.0);
        preset_menu(ui, engine);
        ab_buttons(ui, engine);
        ui.add_space(4.0);
        bypass_button(ui, engine, pal);
        let style = engine.style();
        ui.label(RichText::new(format!("Latency {:.2} ms", engine.latency_ms(style))).color(pal.dim))
            .on_hover_text(format!(
                "{} samples at {:.0} Hz. Hardware styles run 4x oversampled (about 0.6 ms); Clean is zero-latency, or one block plus half the kernel in linear phase. Hosts compensate automatically.",
                engine.latency(),
                engine.sample_rate()
            ));
    });
}

fn style_tabs(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let current = engine.style();
    for s in Style::ALL {
        let sel = s == current;
        let text = RichText::new(s.name()).size(14.0).strong();
        let b = egui::Button::new(if sel {
            text.color(pal.bg)
        } else {
            text.color(pal.text)
        })
        .fill(if sel { pal.sun } else { pal.raised })
        .corner_radius(CornerRadius::same(10));
        if ui.add(b).on_hover_text(info_line(s)).clicked() && !sel {
            engine.set_style(s);
        }
    }
}

fn preset_menu(ui: &mut Ui, engine: &Engine) {
    let current = engine
        .preset()
        .and_then(|i| PRESETS.get(i))
        .map_or("Preset…", |p| p.name);
    egui::ComboBox::from_id_salt("eq-preset")
        .width(150.0)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (i, p) in PRESETS.iter().enumerate() {
                if ui
                    .selectable_label(engine.preset() == Some(i), p.name)
                    .on_hover_text(p.about)
                    .clicked()
                {
                    engine.load_preset(i);
                }
            }
        });
}

fn ab_buttons(ui: &mut Ui, engine: &Engine) {
    let ab = engine.ab();
    for (i, name) in ["A", "B"].into_iter().enumerate() {
        if ui
            .selectable_label(ab == i, RichText::new(name).strong())
            .on_hover_text("A/B compare: two complete setups (style and every knob). Switch to hear the difference.")
            .clicked()
            && ab != i
        {
            engine.select_ab(i);
        }
    }
    if ui
        .small_button(if ab == 0 { "Copy to B" } else { "Copy to A" })
        .on_hover_text("Copy this setup into the other slot.")
        .clicked()
    {
        engine.copy_ab();
    }
}

fn bypass_button(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let id = Pid::g(G::Bypass);
    let on = engine.params().on(id);
    let text = RichText::new("Bypass")
        .strong()
        .color(if on { pal.bg } else { pal.text });
    let b = egui::Button::new(text)
        .fill(if on { pal.warn } else { pal.raised })
        .corner_radius(CornerRadius::same(10));
    if ui
        .add(b)
        .on_hover_text("True bypass: the output is exactly the input (delayed by the latency so the track stays in time).")
        .clicked()
    {
        engine.set(id, if on { 0.0 } else { 1.0 });
    }
}

/// A two-way switch for an on/off parameter.
fn switch(ui: &mut Ui, engine: &Engine, id: Pid, help: &str, pal: &Palette) {
    let v = engine.params().on(id);
    let text = RichText::new(params::format(id, if v { 1.0 } else { 0.0 }))
        .color(if v { pal.bg } else { pal.text });
    let b = egui::Button::new(text)
        .fill(if v { pal.good } else { pal.raised })
        .corner_radius(CornerRadius::same(8));
    if ui.add(b).on_hover_text(help).clicked() {
        engine.set(id, if v { 0.0 } else { 1.0 });
    }
}

/// A draggable value box for a continuous parameter. `shown` overrides the value shown
/// (auto-makeup's live gain); touching it hands control back to the parameter.
fn value_box(ui: &mut Ui, engine: &Engine, id: Pid, help: &str, pal: &Palette, shown: Option<f64>) {
    let Some(info) = id.info() else {
        return;
    };
    let v = shown.unwrap_or_else(|| engine.params().get(id));
    let b = egui::Button::new(RichText::new(params::format(id, v)).color(pal.text))
        .fill(pal.card)
        .min_size(vec2(72.0, 0.0))
        .sense(Sense::click_and_drag());
    let resp = ui
        .add(b)
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(format!(
            "{help}\nDrag or scroll to change, double-click to reset."
        ));
    plate::drag_logic(ui, engine, id, &resp, info.normalize(v), shown.is_some());
}

fn footer(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let style = engine.style();
    let c = |k| Pid::c(style, k);
    let dim = |ui: &mut Ui, s: &str| {
        ui.label(RichText::new(s).color(pal.dim));
    };
    let auto = engine.params().c(style, C::AutoMakeup) >= 0.5;
    ui.horizontal_wrapped(|ui| {
        dim(ui, "Gain In");
        value_box(ui, engine, c(C::GainIn), "Level into the EQ. On the hardware styles, more in = more colour.", pal, None);
        dim(ui, if auto { "Gain Out (auto)" } else { "Gain Out" });
        value_box(
            ui,
            engine,
            c(C::GainOut),
            "Final output level: the makeup gain. MATCH and auto-makeup set it. Touching it turns auto-makeup off.",
            pal,
            auto.then(|| f64::from(engine.meters().auto_makeup())),
        );
        ui.separator();
        dim(ui, "Mix");
        value_box(ui, engine, c(C::Mix), "Blend of EQ'd (wet) and untouched (dry) signal: parallel EQ.", pal, None);
        dim(ui, "Wet");
        value_box(ui, engine, c(C::Wet), "Level of the EQ'd part.", pal, None);
        dim(ui, "Dry");
        value_box(ui, engine, c(C::Dry), "Level of the untouched part (heard when Mix is below 100%).", pal, None);
        ui.separator();
        auto_makeup_switch(ui, engine, pal);
        switch(ui, engine, Pid::g(G::Link), "Linked: dynamic bands use one detector for both channels so the stereo image stays put. Dual mono: each side moves on its own.", pal);
        if C::Colour.in_style(style) {
            switch(ui, engine, c(C::Colour), "Harmonic colour of the circuit, 4x oversampled (no aliasing). Push Gain In for more. Off for the curves alone.", pal);
        }
        if C::Phase.in_style(style) {
            switch(ui, engine, c(C::Phase), "Zero latency: minimum-phase, like an analogue EQ. Linear phase: no phase shift at all, at the cost of latency.", pal);
        }
    });
}

fn auto_makeup_switch(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let on = engine.params().c(engine.style(), C::AutoMakeup) >= 0.5;
    let text = RichText::new(if on { "Auto makeup" } else { "Auto makeup off" }).color(if on {
        pal.bg
    } else {
        pal.text
    });
    let b = egui::Button::new(text)
        .fill(if on { pal.after } else { pal.raised })
        .corner_radius(CornerRadius::same(8));
    if ui
        .add(b)
        .on_hover_text("Gain Out tracks live so the output stays as loud as the input while you turn knobs. Turning it off keeps the gain it reached.")
        .clicked()
    {
        engine.set_auto_makeup(!on);
    }
}

/// The MATCH button and auto-makeup toggle, shared by both side views.
pub(crate) fn match_row(ui: &mut Ui, p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    use egui::{Align2, CursorIcon, FontId, StrokeKind};
    let style = engine.style();
    let auto = engine.params().c(style, C::AutoMakeup) >= 0.5;
    let toggle_w = (r.width() * 0.3).clamp(84.0, 150.0);
    let button = Rect::from_min_max(r.min, pos2(r.right() - toggle_w - 8.0, r.bottom()));
    let toggle = Rect::from_min_max(pos2(button.right() + 8.0, r.top()), r.max);
    let resp = ui
        .interact(button, ui.scope_id().with("eq-match"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let ready = engine.meters().avg_before() > -90.0;
    let fill = if resp.hovered() {
        pal.after
    } else {
        pal.after.gamma_multiply(0.88)
    };
    p.rect_filled(
        button,
        CornerRadius::same(10),
        if ready { fill } else { pal.raised },
    );
    p.text(
        button.center(),
        Align2::CENTER_CENTER,
        "MATCH",
        FontId::proportional((r.height() * 0.5).clamp(15.0, 26.0)),
        if ready { pal.bg } else { pal.dim },
    );
    if resp
        .on_hover_text("Sets Gain Out so the AFTER average peak equals the BEFORE average peak: A/B compares the EQ, not the volume. Play some audio first.")
        .clicked()
    {
        engine.match_now();
    }
    let t = ui
        .interact(toggle, ui.scope_id().with("eq-auto-makeup"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    p.rect_filled(
        toggle,
        CornerRadius::same(10),
        if auto {
            pal.good.gamma_multiply(0.85)
        } else {
            pal.raised
        },
    );
    p.rect_stroke(
        toggle,
        CornerRadius::same(10),
        Stroke::new(1.0, pal.grid),
        StrokeKind::Inside,
    );
    let line2 = if auto {
        format!("{:+.1} dB", engine.meters().auto_makeup())
    } else {
        "off".to_owned()
    };
    p.text(
        toggle.center() - vec2(0.0, 1.0),
        Align2::CENTER_BOTTOM,
        "AUTO MATCH",
        FontId::proportional(11.0),
        if auto { pal.bg } else { pal.text },
    );
    p.text(
        toggle.center() + vec2(0.0, 1.0),
        Align2::CENTER_TOP,
        line2,
        FontId::monospace(12.0),
        if auto { pal.bg } else { pal.dim },
    );
    if t.on_hover_text("Auto makeup: Gain Out tracks live so the output stays as loud as the input. MATCH is the press-once version.")
        .clicked()
    {
        engine.set_auto_makeup(!auto);
    }
}

/// dB readout text.
pub(crate) fn db_text(v: f32) -> String {
    if v <= -90.0 {
        "-inf dB".to_owned()
    } else {
        format!("{v:.1} dB")
    }
}

/// Columns to pixels: the newest column sits at the right edge.
pub(crate) fn column_at(history: &VecDeque<Column>, x: f32, width: f32) -> Option<&Column> {
    let n = history.len();
    if n == 0 {
        return None;
    }
    let back = ((1.0 - x / width) * HISTORY as f32) as usize;
    n.checked_sub(back + 1).and_then(|i| history.get(i))
}

/// A waveform strip (BEFORE or AFTER).
pub(crate) fn waveform(
    p: &egui::Painter,
    r: Rect,
    history: &VecDeque<Column>,
    after: bool,
    pal: &Palette,
) {
    use egui::{Align2, FontId};
    let (title, colour) = if after {
        ("AFTER", pal.after)
    } else {
        ("BEFORE", pal.before)
    };
    p.rect_filled(r, CornerRadius::same(6), pal.card);
    let mid = r.center().y;
    let half = r.height() * 0.42;
    p.line_segment(
        [pos2(r.left(), mid), pos2(r.right(), mid)],
        Stroke::new(1.0, pal.grid),
    );
    let w = r.width().max(1.0);
    let mut x = 0.0;
    while x < w {
        if let Some(c) = column_at(history, x, w) {
            let (lo, hi) = if after { c.after } else { c.before };
            let y0 = mid - hi.clamp(-1.0, 1.0) * half;
            let y1 = mid - lo.clamp(-1.0, 1.0) * half;
            p.line_segment(
                [pos2(r.left() + x, y0), pos2(r.left() + x, y1.max(y0 + 1.0))],
                Stroke::new(1.0, colour),
            );
        }
        x += 1.0;
    }
    let small = r.height() < 44.0;
    p.circle_filled(r.left_top() + vec2(10.0, 11.0), 3.5, colour);
    p.text(
        r.left_top() + vec2(18.0, 4.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(if small { 10.5 } else { 13.0 }),
        colour,
    );
    if !small {
        p.text(
            r.right_top() + vec2(-8.0, 5.0),
            Align2::RIGHT_TOP,
            if after { "wet output" } else { "dry input" },
            FontId::proportional(11.0),
            pal.dim,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(engine: &Engine, local: &mut UiLocal, size: (f32, f32)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(size.0, size.1))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| show(ui, engine, local));
        // Nothing renders in a test: drop the font atlas upload on purpose.
        out.textures_delta.clear();
        out
    }

    #[test]
    fn every_style_draws_at_default_and_minimum_size() {
        let engine = Engine::new();
        engine.load_preset(PRESETS.iter().position(|p| p.name == "Vocals").unwrap_or(0));
        for s in Style::ALL {
            engine.set_style(s);
            for size in [DEFAULT_SIZE, MIN_SIZE] {
                let mut local = UiLocal::default();
                let out = frame(&engine, &mut local, size);
                assert!(!out.shapes.is_empty(), "{s:?} {size:?}");
            }
        }
    }

    #[test]
    fn every_spectrometer_view_draws() {
        let engine = Engine::new();
        engine.set_style(Style::Clean);
        for v in [SpecView::Overlay, SpecView::SideBySide, SpecView::Stacked] {
            let mut local = UiLocal {
                spec_view: v,
                ..UiLocal::default()
            };
            let out = frame(&engine, &mut local, DEFAULT_SIZE);
            assert!(!out.shapes.is_empty());
        }
    }

    #[test]
    fn egui_has_fonts() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let has = ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), 'S'));
        assert!(has);
    }

    #[test]
    fn info_lines_name_the_unit_and_history() {
        assert!(info_line(Style::Pultec).contains("EQP-1A"));
        assert!(info_line(Style::Api).contains("550A"));
        assert!(info_line(Style::Ssl).contains("G-Series"));
        assert!(info_line(Style::Neve).contains("1073"));
        assert!(info_line(Style::Massive).contains("Massive Passive"));
        assert!(info_line(Style::Maag).contains("EQ4"));
        assert!(info_line(Style::Curve).contains("Curve Bender"));
        assert!(info_line(Style::Clean).contains("no hardware"));
        for s in Style::ALL {
            assert!(info_line(s).contains("Best on") || info_line(s).contains("Best for"));
        }
    }
}
