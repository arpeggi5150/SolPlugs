//! The editor, drawn with egui. [`show`] draws everything into a `Ui`; the plugin calls it
//! from its own window, Solara and the standalone app from a panel.
//!
//! Layout: a header (☀ SolPlugs, mode tabs, presets, A/B, bypass, latency) with the
//! mode's plain-words info line under it; the body has the mode's hardware faceplate on
//! the left and the flat digital side view on the right; a footer holds the detector,
//! link and sidechain switches.

mod digital;
mod faceplates;
mod plate;
pub mod theme;

use std::collections::VecDeque;

use egui::{Color32, CornerRadius, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use solplugs_comp_core::Mode;

use crate::engine::{Column, Engine};
use crate::params::{self, G, K, Pid};
use crate::presets::PRESETS;
use faceplates::MeterView;
use theme::Palette;

/// Default editor size, logical pixels.
pub const DEFAULT_SIZE: (f32, f32) = (1200.0, 780.0);
/// Smallest editor size that stays usable, logical pixels.
pub const MIN_SIZE: (f32, f32) = (900.0, 600.0);

/// About five seconds of display columns.
const HISTORY: usize = 1000;

/// UI-only state: the waveform history and the needle ballistics.
#[derive(Default)]
pub struct UiLocal {
    styled: bool,
    history: VecDeque<Column>,
    needle: [f32; 2],
    needle_primed: bool,
}

/// The plain-words line for each mode: what it is, what it's styled after, what it was
/// used on.
pub fn info_line(mode: Mode) -> &'static str {
    match mode {
        Mode::Vca => {
            "VCA: fast, punchy and clean, the glue compressor. Styled after the SSL G bus comp (with a nod to the API 2500 and Neve 33609): the mix-bus glue on countless records from the 80s on. Best on the mix bus, drums and groups."
        }
        Mode::Opto => {
            "Opto: smooth, slow and musical, a light-cell leveller that never grabs. Styled after the Teletronix LA-2A from the 1960s: the classic vocal leveller of Motown, crooners and smooth bass. Best on vocals and bass."
        }
        Mode::Fet => {
            "FET: fast, aggressive and gritty, it grabs hard and adds bite. Styled after the UREI 1176 from 1967 (and the Distressor): the rock vocal and drum smasher. Press ALL for the famous all-buttons-in smash. Best on drums and vocals that need attitude."
        }
        Mode::VariMu => {
            "Vari-Mu: warm tube glue, slow and soft, holds a mix together. Styled after the Fairchild 670 from the 1950s (and the Manley Variable Mu): the mastering and bus compressor of the vinyl era at Abbey Road. Best on the mix bus and mastering."
        }
        Mode::Clean => {
            "Clean: transparent modern digital, no colour, just control. Styled after no hardware: the precise DAW-era compressor, with lookahead so no peak slips through. Best on anything that must stay exactly as it is."
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
    let view = local.meter_view(ui, engine);

    egui::Panel::top("comp-top")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| {
            top_bar(ui, engine, &pal);
            ui.add(
                egui::Label::new(
                    RichText::new(info_line(engine.mode()))
                        .color(pal.dim)
                        .size(12.5),
                )
                .wrap(),
            );
        });
    egui::Panel::bottom("comp-footer")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| footer(ui, engine, &pal));
    egui::CentralPanel::default()
        .frame(theme::body_frame(&pal))
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let (left, right) = split(area);
            faceplates::show(ui, left, engine, &view);
            digital::show(ui, right, engine, &local.history, &pal);
        });
}

/// Splits the body: the faceplate gets the width its proportions want (at most 55%), the
/// side view the rest.
fn split(area: Rect) -> (Rect, Rect) {
    let want = area.height() * faceplates::CANVAS.0 / faceplates::CANVAS.1;
    let w = want.min(area.width() * 0.55).max(area.width() * 0.4);
    let left = Rect::from_min_size(area.min, vec2(w, area.height()));
    let right = Rect::from_min_max(pos2(left.right() + 10.0, area.top()), area.max);
    (left, right)
}

impl UiLocal {
    /// Needles move like a VU meter (about 300 ms), from the engine's gain reduction.
    fn meter_view(&mut self, ui: &Ui, engine: &Engine) -> MeterView {
        let (l, r) = engine.meters().gr();
        let link = engine.params().global().link;
        let target = if link {
            let g = faceplates::needle_for(l.max(r));
            [g, g]
        } else {
            [faceplates::needle_for(l), faceplates::needle_for(r)]
        };
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
        let k = 1.0 - (-dt / 0.065).exp();
        for (n, t) in self.needle.iter_mut().zip(target) {
            *n = if self.needle_primed {
                *n + (t - *n) * k
            } else {
                t
            };
        }
        self.needle_primed = true;
        MeterView {
            needle: self.needle,
            gr: l.max(r),
        }
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
        ui.label(RichText::new("Compressor").strong().color(pal.text).size(15.0));
        ui.add_space(10.0);
        mode_tabs(ui, engine, pal);
        ui.add_space(10.0);
        preset_menu(ui, engine);
        ab_buttons(ui, engine);
        ui.add_space(6.0);
        bypass_button(ui, engine, pal);
        let mode = engine.mode();
        ui.label(RichText::new(format!("Latency {:.2} ms", engine.latency_ms(mode))).color(pal.dim))
            .on_hover_text(format!(
                "{} samples at {:.0} Hz. Coloured modes run 4x oversampled; Clean adds only its lookahead. Hosts compensate automatically.",
                engine.latency(),
                engine.sample_rate()
            ));
    });
}

fn mode_tabs(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let current = engine.mode();
    for m in Mode::ALL {
        let sel = m == current;
        let text = RichText::new(m.name()).size(14.0).strong();
        let b = egui::Button::new(if sel {
            text.color(pal.bg)
        } else {
            text.color(pal.text)
        })
        .fill(if sel { pal.sun } else { pal.raised })
        .corner_radius(CornerRadius::same(10));
        if ui.add(b).on_hover_text(info_line(m)).clicked() && !sel {
            engine.set_mode(m);
        }
    }
}

fn preset_menu(ui: &mut Ui, engine: &Engine) {
    let current = engine
        .preset()
        .and_then(|i| PRESETS.get(i))
        .map_or("Preset…", |p| p.name);
    egui::ComboBox::from_id_salt("comp-preset")
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
            .on_hover_text("A/B compare: two complete setups (mode and every knob). Switch to hear the difference.")
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
fn switch(
    ui: &mut Ui,
    engine: &Engine,
    id: Pid,
    (off, on): (&str, &str),
    help: &str,
    pal: &Palette,
) {
    let v = engine.params().on(id);
    let text = RichText::new(if v { on } else { off }).color(if v { pal.bg } else { pal.text });
    let b = egui::Button::new(text)
        .fill(if v { pal.good } else { pal.raised })
        .corner_radius(CornerRadius::same(8));
    if ui.add(b).on_hover_text(help).clicked() {
        engine.set(id, if v { 0.0 } else { 1.0 });
    }
}

/// A draggable value box for a continuous parameter.
fn value_box(ui: &mut Ui, engine: &Engine, id: Pid, help: &str, pal: &Palette) {
    let Some(info) = id.info() else {
        return;
    };
    let v = engine.params().get(id);
    let b = egui::Button::new(RichText::new(params::format(id, v)).color(pal.text))
        .fill(pal.card)
        .min_size(vec2(70.0, 0.0))
        .sense(Sense::click_and_drag());
    let resp = ui
        .add(b)
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(format!(
            "{help}\nDrag or scroll to change, double-click to reset."
        ));
    plate::drag_logic(ui, engine, id, &resp, info.normalize(v), false);
}

fn footer(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let mode = engine.mode();
    let m = |k| Pid::m(mode, k);
    let dim = |ui: &mut Ui, s: &str| {
        ui.label(RichText::new(s).color(pal.dim));
    };
    ui.horizontal_wrapped(|ui| {
        dim(ui, "Detector");
        switch(ui, engine, m(K::Detector), ("Peak", "RMS"), "Peak follows every transient; RMS follows loudness, smoother.", pal);
        switch(ui, engine, m(K::AutoRelease), ("Auto release off", "Auto release"), "Program-dependent release: quick after short peaks, slow after long loud passages.", pal);
        ui.separator();
        dim(ui, "Stereo");
        switch(ui, engine, Pid::g(G::Link), ("Dual mono", "Linked"), "Linked: one detector drives both sides so the stereo image stays put. Dual mono: each side compresses on its own.", pal);
        ui.separator();
        dim(ui, "SC HPF");
        value_box(ui, engine, m(K::ScHpf), "Sidechain high-pass: stops bass from pumping the compressor. The audio itself is not filtered.", pal);
        switch(ui, engine, Pid::g(G::ExtSc), ("Int SC", "Ext SC 3/4"), "External sidechain: the detector listens to inputs 3/4 (route a key track there) instead of the main input.", pal);
        ui.separator();
        if K::Colour.in_mode(mode) {
            switch(ui, engine, m(K::Colour), ("Colour off", "Colour"), "Harmonic colour of the circuit (oversampled, no aliasing). Turn off for the dynamics alone.", pal);
        }
        if K::Lookahead.in_mode(mode) {
            dim(ui, "Lookahead");
            value_box(ui, engine, m(K::Lookahead), "Clean only: sees peaks coming so none slip through. Adds this much latency.", pal);
        }
        auto_makeup_switch(ui, engine, pal);
    });
}

fn auto_makeup_switch(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let on = engine.params().m(engine.mode(), K::AutoMakeup) >= 0.5;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(engine: &Engine, local: &mut UiLocal, size: (f32, f32)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(size.0, size.1))),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| show(ui, engine, local))
    }

    #[test]
    fn every_mode_draws_at_default_and_minimum_size() {
        let engine = Engine::new();
        for m in Mode::ALL {
            engine.set_mode(m);
            for size in [DEFAULT_SIZE, MIN_SIZE] {
                let mut local = UiLocal::default();
                let out = frame(&engine, &mut local, size);
                assert!(!out.shapes.is_empty(), "{m:?} {size:?}");
            }
        }
    }

    #[test]
    fn egui_has_fonts() {
        // Without egui's default_fonts feature every glyph is a box.
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let has = ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), 'S'));
        assert!(has);
    }

    #[test]
    fn info_lines_name_the_unit() {
        assert!(info_line(Mode::Fet).contains("1176"));
        assert!(info_line(Mode::Opto).contains("LA-2A"));
        assert!(info_line(Mode::VariMu).contains("Fairchild"));
        assert!(info_line(Mode::Vca).contains("SSL"));
        assert!(info_line(Mode::Clean).contains("no hardware"));
    }
}
