//! The editor, drawn with egui. [`show`] draws everything into a `Ui`; the plugin calls it
//! from its own window, Solara and the standalone app from a panel.
//!
//! Layout (locked): a header (☀ SolPlugs Delay, version tabs, presets, A/B, bypass, view
//! toggle, latency) with the version's plain-words info line under it, and a footer with
//! ducking, the stereo and ping-pong stages, modulation and the output (Gain In/Out, Mix,
//! Wet, Dry, stereo link). The body is either
//! - **digital-first** (default): the big delay window with the BEFORE/AFTER waveforms
//!   (left and right with their meters), post meter and MATCH, the time section (TIME knob,
//!   BPM display, SYNC, TAP TEMPO) and the repeats picture with the feedback meter, and a
//!   slim hardware strip along the bottom; or
//! - **stacked**: the hardware panel on top, the digital view below.

mod digital;
mod hardware;
mod plate;
pub mod theme;

use std::collections::VecDeque;

use egui::{Color32, CornerRadius, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use solplugs_delay_core::{Ctl, Version};

use crate::engine::{Column, Engine};
use crate::params::{self, G, Pid};
use crate::presets::PRESETS;
use theme::Palette;

/// Default editor size, logical pixels.
pub const DEFAULT_SIZE: (f32, f32) = (1240.0, 820.0);
/// Smallest editor size that stays usable, logical pixels.
pub const MIN_SIZE: (f32, f32) = (860.0, 600.0);

/// About five seconds of display columns.
const HISTORY: usize = 1000;

/// UI-only state: waveform history.
#[derive(Default)]
pub struct UiLocal {
    styled: bool,
    history: VecDeque<Column>,
}

/// The plain-words line for each version: what it is, what it's styled after, what it's
/// good on.
pub fn info_line(v: Version) -> &'static str {
    match v {
        Version::Clean => {
            "Clean: modern digital delay, clear repeats, exact timing, no colour. Styled after no hardware: every repeat is a perfect copy until the filters say otherwise. Best on vocals, rhythmic guitar parts and anything that has to sit exactly on the beat."
        }
        Version::Tape => {
            "Tape: a tape echo with two reels. Styled after the classic tape echo units: each repeat comes back warmer and darker, with a little wow and flutter, and extra playback heads for galloping rhythms. Best on vocals, guitars, dub and anything that wants vintage warmth."
        }
        Version::Analog => {
            "Analog: a dark bucket-brigade pedal. Styled after the classic analog delay pedals: the repeats get darker fast and melt away, with a slow chorus on top. Best on guitars, synths and backing vocals that should sit behind the mix."
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

    egui::Panel::top("dl-top")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| {
            top_bar(ui, engine, &pal);
            ui.add(
                egui::Label::new(
                    RichText::new(info_line(engine.version()))
                        .color(pal.dim)
                        .size(12.5),
                )
                .wrap(),
            );
        });
    egui::Panel::bottom("dl-footer")
        .frame(theme::bar_frame(&pal))
        .show(ui, |ui| footer(ui, engine, &pal));
    egui::CentralPanel::default()
        .frame(theme::body_frame(&pal))
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            if engine.stacked() {
                // Hardware on top at its natural shape (up to 45% of the height), digital below.
                let want = area.width() * hardware::PANEL_H / hardware::PANEL_W;
                let h = want.min(area.height() * 0.45);
                let top = Rect::from_min_size(area.min, vec2(area.width(), h));
                let rest = Rect::from_min_max(pos2(area.left(), top.bottom() + 8.0), area.max);
                hardware::show(ui, top, engine, true);
                digital::show(ui, rest, engine, local, &pal);
            } else {
                // Digital first; the slim strip along the bottom.
                let want = area.width() * hardware::STRIP_H / hardware::STRIP_W;
                let h = want.clamp(72.0, area.height() * 0.24);
                let strip = Rect::from_min_max(pos2(area.left(), area.bottom() - h), area.max);
                let rest = Rect::from_min_max(area.min, pos2(area.right(), strip.top() - 8.0));
                digital::show(ui, rest, engine, local, &pal);
                hardware::show(ui, strip, engine, false);
            }
        });
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
        ui.label(RichText::new("Delay").strong().color(pal.text).size(15.0));
        ui.add_space(8.0);
        version_tabs(ui, engine, pal);
        ui.add_space(8.0);
        preset_menu(ui, engine);
        ab_buttons(ui, engine);
        ui.add_space(4.0);
        bypass_button(ui, engine, pal);
        view_toggle(ui, engine, pal);
        let v = engine.version();
        ui.label(RichText::new(format!("Latency {:.2} ms", engine.latency_ms(v))).color(pal.dim))
            .on_hover_text(format!(
                "Measured {} samples at {:.0} Hz on {}: every version is zero-latency (the dry path is never delayed). Hosts need no compensation.",
                engine.latency(),
                engine.sample_rate(),
                v.name()
            ));
    });
}

fn version_tabs(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let current = engine.version();
    for v in Version::ALL {
        let sel = v == current;
        let text = RichText::new(v.name()).size(14.0).strong();
        let b = egui::Button::new(if sel {
            text.color(pal.bg)
        } else {
            text.color(pal.text)
        })
        .fill(if sel { pal.sun } else { pal.raised })
        .corner_radius(CornerRadius::same(10));
        if ui.add(b).on_hover_text(info_line(v)).clicked() && !sel {
            engine.set_version(v);
        }
    }
}

fn preset_menu(ui: &mut Ui, engine: &Engine) {
    let current = engine
        .preset()
        .and_then(|i| PRESETS.get(i))
        .map_or("Preset…", |p| p.name);
    egui::ComboBox::from_id_salt("dl-preset")
        .width(130.0)
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
            .on_hover_text("A/B compare: two complete setups (version and every knob). Switch to hear the difference.")
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
        .on_hover_text("True bypass: the output is exactly the input.")
        .clicked()
    {
        engine.set(id, if on { 0.0 } else { 1.0 });
    }
}

fn view_toggle(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let stacked = engine.stacked();
    for (on, label) in [(false, "Digital"), (true, "Stacked")] {
        let sel = stacked == on;
        let b = egui::Button::new(RichText::new(label).color(if sel { pal.bg } else { pal.text }))
            .fill(if sel { pal.before } else { pal.raised })
            .corner_radius(CornerRadius::same(8));
        if ui
            .add(b)
            .on_hover_text("Digital: the big delay window with a slim hardware strip at the bottom. Stacked: the hardware on top, the digital view below.")
            .clicked()
        {
            engine.set_stacked(on);
        }
    }
}

/// A two-way switch for an on/off parameter.
fn switch(ui: &mut Ui, engine: &Engine, id: Pid, pal: &Palette) {
    let v = engine.params().on(id);
    let text = RichText::new(params::format(id, if v { 1.0 } else { 0.0 })).color(if v {
        pal.bg
    } else {
        pal.text
    });
    let b = egui::Button::new(text)
        .fill(if v { pal.good } else { pal.raised })
        .corner_radius(CornerRadius::same(8));
    let help = id.info().map_or("", |i| i.help);
    if ui.add(b).on_hover_text(help).clicked() {
        engine.set(id, if v { 0.0 } else { 1.0 });
    }
}

/// A draggable value box for a continuous parameter. `shown` overrides the value shown
/// (auto-makeup's live gain); touching it hands control back to the parameter.
fn value_box(ui: &mut Ui, engine: &Engine, id: Pid, pal: &Palette, shown: Option<f64>) {
    let Some(info) = id.info() else {
        return;
    };
    let v = shown.unwrap_or_else(|| engine.params().get(id));
    let b = egui::Button::new(RichText::new(params::format(id, v)).color(pal.text))
        .fill(pal.card)
        .min_size(vec2(62.0, 0.0))
        .sense(Sense::click_and_drag());
    let resp = ui
        .add(b)
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(format!(
            "{}: {}\nDrag or scroll to change, double-click to reset.",
            info.name, info.help
        ));
    plate::drag_logic(ui, engine, id, &resp, info.normalize(v), shown.is_some());
}

/// A group title in the footer. The row wraps between any two items.
fn title(ui: &mut Ui, text: &str, pal: &Palette) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).color(pal.before).size(11.5).strong());
}

fn footer(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let v = engine.version();
    let c = |k| Pid::c(v, k);
    let dim = |ui: &mut Ui, s: &str| {
        ui.label(RichText::new(s).color(pal.dim).size(11.5));
    };
    let auto = engine.params().c(v, Ctl::AutoMakeup) >= 0.5;
    ui.horizontal_wrapped(|ui| {
        title(ui, "DUCKING", pal);
        {
            value_box(ui, engine, c(Ctl::Duck), pal, None);
            dim(ui, "Rel");
            value_box(ui, engine, c(Ctl::DuckRelease), pal, None);
            dim(ui, "Key");
            switch(ui, engine, c(Ctl::DuckSource), pal);
            let d = engine.meters().duck();
            ui.label(
                RichText::new(format!("GR {d:.1} dB"))
                    .monospace()
                    .color(if d > 0.3 { pal.gr } else { pal.dim }),
            );
        }
        title(ui, "STEREO", pal);
        {
            dim(ui, "Wet");
            value_box(ui, engine, c(Ctl::StereoMix), pal, None);
            dim(ui, "Width");
            value_box(ui, engine, c(Ctl::Width), pal, None);
            dim(ui, "L/R Offset");
            value_box(ui, engine, c(Ctl::Offset), pal, None);
        }
        title(ui, "PING-PONG", pal);
        {
            dim(ui, "Wet");
            value_box(ui, engine, c(Ctl::PingPongMix), pal, None);
        }
        if v.has_mod() {
            title(
                ui,
                if v == Version::Tape {
                    "WOW / FLUTTER"
                } else {
                    "CHORUS"
                },
                pal,
            );
            value_box(ui, engine, c(Ctl::Mod), pal, None);
            dim(ui, "Rate");
            value_box(ui, engine, c(Ctl::ModRate), pal, None);
        }
        title(ui, "OUTPUT", pal);
        {
            dim(ui, "In");
            value_box(ui, engine, c(Ctl::GainIn), pal, None);
            dim(ui, if auto { "Out (auto)" } else { "Out" });
            value_box(
                ui,
                engine,
                c(Ctl::GainOut),
                pal,
                auto.then(|| f64::from(engine.meters().auto_makeup())),
            );
            dim(ui, "Mix");
            value_box(ui, engine, c(Ctl::Mix), pal, None);
            dim(ui, "Wet");
            value_box(ui, engine, c(Ctl::Wet), pal, None);
            dim(ui, "Dry");
            value_box(ui, engine, c(Ctl::Dry), pal, None);
        }
        switch(ui, engine, Pid::g(G::Link), pal);
    });
}

/// The MATCH button and auto-makeup toggle.
pub(crate) fn match_row(ui: &mut Ui, p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    use egui::{Align2, CursorIcon, FontId, StrokeKind};
    let v = engine.version();
    let auto = engine.params().c(v, Ctl::AutoMakeup) >= 0.5;
    let toggle_w = (r.width() * 0.34).clamp(70.0, 140.0);
    let button = Rect::from_min_max(r.min, pos2(r.right() - toggle_w - 6.0, r.bottom()));
    let toggle = Rect::from_min_max(pos2(button.right() + 6.0, r.top()), r.max);
    let resp = ui
        .interact(button, ui.scope_id().with("dl-match"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let ready = engine.meters().avg_before() > -90.0;
    let fill = if resp.hovered() {
        pal.accent
    } else {
        pal.accent.gamma_multiply(0.85)
    };
    p.rect_filled(
        button,
        CornerRadius::same(8),
        if ready { fill } else { pal.raised },
    );
    p.text(
        button.center(),
        Align2::CENTER_CENTER,
        "MATCH",
        FontId::proportional((r.height() * 0.5).clamp(13.0, 26.0)),
        if ready { pal.bg } else { pal.dim },
    );
    if resp
        .on_hover_text("Sets Gain Out so the AFTER average peak equals the BEFORE average peak: A/B compares the delay, not the volume. Play some audio first.")
        .clicked()
    {
        engine.match_now();
    }
    let t = ui
        .interact(toggle, ui.scope_id().with("dl-auto"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    p.rect_filled(
        toggle,
        CornerRadius::same(8),
        if auto {
            pal.good.gamma_multiply(0.85)
        } else {
            pal.raised
        },
    );
    p.rect_stroke(
        toggle,
        CornerRadius::same(8),
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
        "AUTO",
        FontId::proportional(11.0),
        if auto { pal.bg } else { pal.text },
    );
    p.text(
        toggle.center() + vec2(0.0, 1.0),
        Align2::CENTER_TOP,
        line2,
        FontId::monospace(11.5),
        if auto { pal.bg } else { pal.dim },
    );
    if t.on_hover_text("Auto makeup: Gain Out tracks live so the output stays as loud as the input. MATCH is the press-once version.").clicked() {
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
        out.textures_delta.clear();
        out
    }

    #[test]
    fn every_version_draws_in_both_views_at_default_and_minimum_size() {
        let engine = Engine::new();
        for v in Version::ALL {
            engine.set_version(v);
            for stacked in [false, true] {
                engine.set_stacked(stacked);
                for size in [DEFAULT_SIZE, MIN_SIZE] {
                    let mut local = UiLocal::default();
                    let out = frame(&engine, &mut local, size);
                    assert!(!out.shapes.is_empty(), "{v:?} {size:?}");
                }
            }
        }
    }

    /// Every text drawn in a frame, with its font size.
    fn texts(out: &egui::FullOutput) -> Vec<(String, f32)> {
        fn walk(s: &egui::Shape, acc: &mut Vec<(String, f32)>) {
            match s {
                egui::Shape::Text(t) => {
                    let size = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map_or(0.0, |x| x.format.font_id.size);
                    acc.push((t.galley.text().to_owned(), size));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                _ => {}
            }
        }
        let mut acc = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut acc);
        }
        acc
    }

    /// Test 17: the BPM is the largest number, the ms and the note value smaller below it.
    /// Test 11: still there and legible at the smallest size.
    #[test]
    #[allow(clippy::expect_used)]
    fn bpm_is_the_biggest_number() {
        let engine = Engine::new();
        for size in [DEFAULT_SIZE, MIN_SIZE] {
            let mut local = UiLocal::default();
            frame(&engine, &mut local, size);
            let out = frame(&engine, &mut local, size);
            let t = texts(&out);
            let bpm = t.iter().find(|(s, _)| s == "120").expect("BPM shown");
            let ms = t
                .iter()
                .find(|(s, _)| s.contains("375 ms") && s.contains("1/8 dotted"))
                .expect("ms and note shown");
            let biggest = t.iter().map(|(_, z)| *z).fold(0.0, f32::max);
            assert_eq!(bpm.1, biggest, "{size:?}");
            assert!(ms.1 < bpm.1 && ms.1 >= plate::MIN_TEXT, "{size:?}");
            assert!(t.iter().all(|(_, z)| *z >= 9.0), "{size:?}: tiny text");
        }
    }

    #[test]
    fn egui_has_fonts() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        assert!(ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), 'S')));
    }

    #[test]
    fn info_lines_name_the_type_never_a_brand() {
        assert!(info_line(Version::Tape).contains("tape echo"));
        assert!(info_line(Version::Analog).contains("bucket-brigade"));
        assert!(info_line(Version::Clean).contains("no hardware"));
        for v in Version::ALL {
            assert!(info_line(v).contains("Best on"));
            for brand in [
                "Roland",
                "Space Echo",
                "RE-201",
                "H-Delay",
                "Waves",
                "Boss",
                "Memory Man",
            ] {
                assert!(!info_line(v).contains(brand), "{v:?}");
            }
        }
    }
}
