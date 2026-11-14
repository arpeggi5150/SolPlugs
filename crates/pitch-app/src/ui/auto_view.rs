//! Automatic mode: note readout, meters, the live pitch trail, the controls and the scale
//! keyboard.

use egui::{
    Align2, CornerRadius, FontId, RichText, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use solplugs_pitch_core::scale::{NOTE_NAMES, ScaleKind, note_name};

use super::UiLocal;
use super::theme::{self, Palette};
use super::widgets;
use crate::engine::Engine;
use crate::params::P;

/// Trail length, frames (~4 s).
pub const TRAIL_LEN: usize = 1500;

pub fn show(ui: &mut Ui, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let m = engine.meters();
    let top_h = (ui.available_height() - 250.0).max(230.0);
    ui.horizontal(|ui| {
        theme::card(pal).show(ui, |ui| {
            ui.set_width(210.0);
            ui.set_height(top_h);
            ui.vertical(|ui| readout(ui, engine, pal, m.input(), m.output(), m.note()));
        });
        theme::card(pal).show(ui, |ui| {
            let w = ui.available_width();
            ui.set_height(top_h);
            trail(ui, engine, local, pal, w, top_h);
        });
    });
    ui.add_space(8.0);
    theme::card(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::knob(ui, engine, P::Retune, pal, "How fast notes are pulled to pitch. 0 ms = hard snap (the robotic effect); 50-150 ms sounds natural; slower lets slides and scoops through.");
            widgets::knob(ui, engine, P::Humanize, pal, "Relaxes the correction on held notes so their natural movement stays, while short notes are still corrected at the retune speed.");
            widgets::knob(ui, engine, P::Amount, pal, "How much of the correction is applied. 0% leaves the voice exactly as sung.");
            widgets::knob(ui, engine, P::Vibrato, pal, "Vibrato depth in the output: 100% keeps it as sung, 0% flattens it, 200% doubles it. (At retune 0 the vibrato is flattened anyway.)");
            widgets::knob(ui, engine, P::Transpose, pal, "Shifts the corrected voice by whole semitones.");
            ui.separator();
            ui.vertical(|ui| {
                ui.add_space(6.0);
                widgets::toggle(ui, engine, P::Formant, "Formant", pal.good, pal, "Formant preservation (on by default): the voice keeps its character when shifted. Off gives the chipmunk / giant effect.");
            });
            widgets::knob(ui, engine, P::FormantShift, pal, "Moves the formants (the throat size) without changing the pitch.");
        });
    });
    ui.add_space(8.0);
    theme::card(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        scale_keys(ui, engine, pal);
    });
}

fn readout(ui: &mut Ui, engine: &Engine, pal: &Palette, input: f32, output: f32, note: i32) {
    ui.label(RichText::new("INPUT").color(pal.text_dim).size(11.0));
    let voiced = input > 0.0;
    let near = input.round() as i32;
    let cents = (input - near as f32) * 100.0;
    let (rect, _) = ui.allocate_exact_size(vec2(190.0, 64.0), Sense::hover());
    let p = ui.painter_at(rect);
    let colour = if !voiced {
        pal.text_dim
    } else if cents.abs() < 10.0 {
        pal.good
    } else {
        pal.input
    };
    p.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        if voiced { note_name(near) } else { "-".into() },
        FontId::proportional(48.0),
        colour,
    );
    if voiced {
        p.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            format!("{cents:+.0}¢"),
            FontId::proportional(22.0),
            pal.text,
        );
    }
    ui.label(
        RichText::new("Pitch vs nearest note (±50¢)")
            .color(pal.text_dim)
            .size(11.0),
    );
    widgets::centre_meter(ui, voiced.then_some(cents / 50.0), 190.0, colour, pal);
    ui.add_space(10.0);
    ui.label(RichText::new("CORRECTION").color(pal.text_dim).size(11.0));
    let shift = if voiced { output - input } else { 0.0 };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(if voiced && note >= 0 {
                format!("to {}", note_name(note))
            } else {
                "-".into()
            })
            .size(18.0)
            .color(pal.output),
        );
        ui.label(
            RichText::new(format!("{:+.0}¢", shift * 100.0))
                .size(14.0)
                .color(pal.text),
        );
    });
    widgets::centre_meter(ui, voiced.then_some(shift), 190.0, pal.output, pal);
    ui.label(RichText::new("±1 semitone").color(pal.text_dim).size(10.0));
    ui.add_space(6.0);
    let level = engine.meters().level();
    let db = 20.0 * level.max(1e-6).log10();
    ui.label(
        RichText::new(format!("Input level {db:.0} dBFS"))
            .color(pal.text_dim)
            .size(11.0),
    );
}

fn trail(
    ui: &mut Ui,
    engine: &Engine,
    local: &mut UiLocal,
    pal: &Palette,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let p = ui.painter_at(rect);
    // Follow the singer: centre on the recent median pitch, smoothly.
    let mut recent: Vec<f32> = local
        .trail
        .iter()
        .rev()
        .take(400)
        .map(|t| t.input)
        .filter(|m| *m > 0.0)
        .collect();
    if !recent.is_empty() {
        recent.sort_by(f32::total_cmp);
        let med = recent[recent.len() / 2];
        if local.trail_centre == 0.0 {
            local.trail_centre = med;
        }
        local.trail_centre += 0.05 * (med - local.trail_centre);
    } else if local.trail_centre == 0.0 {
        local.trail_centre = 60.0;
    }
    let span = 14.0;
    let top = local.trail_centre + span / 2.0;
    let y = |m: f32| rect.top() + (top - m) / span * rect.height();
    let scale = engine.scale();
    let lo = (top - span).floor() as i32;
    for n in lo..=(top.ceil() as i32) {
        let yy = y(n as f32);
        if yy < rect.top() || yy > rect.bottom() {
            continue;
        }
        let in_scale = scale.contains(n);
        p.line_segment(
            [pos2(rect.left() + 34.0, yy), pos2(rect.right(), yy)],
            Stroke::new(
                if in_scale { 1.0 } else { 0.5 },
                if in_scale { pal.grid } else { pal.panel_hi },
            ),
        );
        if in_scale {
            p.text(
                pos2(rect.left(), yy),
                Align2::LEFT_CENTER,
                note_name(n),
                FontId::proportional(10.0),
                pal.text_dim,
            );
        }
    }
    let n = local.trail.len();
    let dx = (rect.width() - 36.0) / TRAIL_LEN as f32;
    let x0 = rect.right() - n as f32 * dx;
    for (sel, colour, w) in [(0, pal.input, 1.5), (1, pal.output, 2.0)] {
        let mut run: Vec<egui::Pos2> = Vec::new();
        for (i, t) in local.trail.iter().enumerate() {
            let m = if sel == 0 { t.input } else { t.output };
            if m > 0.0 {
                run.push(pos2(x0 + i as f32 * dx, y(m)));
            } else if run.len() > 1 {
                p.add(Shape::line(
                    std::mem::take(&mut run),
                    Stroke::new(w, colour),
                ));
            } else {
                run.clear();
            }
        }
        if run.len() > 1 {
            p.add(Shape::line(run, Stroke::new(w, colour)));
        }
    }
    p.text(
        rect.right_top() + vec2(-6.0, 4.0),
        Align2::RIGHT_TOP,
        "input  ",
        FontId::proportional(11.0),
        pal.input,
    );
    p.text(
        rect.right_top() + vec2(-6.0, 18.0),
        Align2::RIGHT_TOP,
        "corrected",
        FontId::proportional(11.0),
        pal.output,
    );
}

fn scale_keys(ui: &mut Ui, engine: &Engine, pal: &Palette) {
    let scale = engine.scale();
    ui.horizontal(|ui| {
        ui.label(RichText::new("NOTES").color(pal.text_dim).size(11.0))
            .on_hover_text("Lit notes are the ones voices are tuned to. Click a note to switch it on or off (this makes a custom scale).");
        for (i, name) in NOTE_NAMES.iter().enumerate() {
            let on = scale.mask & (1 << i) != 0;
            let black = name.contains('#');
            let size = vec2(38.0, 30.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter_at(rect);
            let fill = if on {
                pal.output
            } else if black {
                pal.bg
            } else {
                pal.panel_hi
            };
            p.rect_filled(rect, CornerRadius::same(4), fill);
            if resp.hovered() {
                p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal.text), StrokeKind::Inside);
            }
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                *name,
                FontId::proportional(12.0),
                if on { pal.bg } else { pal.text_dim },
            );
            if resp.clicked() {
                let mask = scale.mask ^ (1 << i);
                if mask != 0 {
                    engine.set_custom_mask(mask);
                    engine.set(P::Scale, ScaleKind::Custom.index() as f64);
                }
            }
        }
    });
}
