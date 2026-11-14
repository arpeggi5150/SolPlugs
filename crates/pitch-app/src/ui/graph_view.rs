//! Graph mode: the captured take as note blobs on a piano roll, with the pitch curve.
//!
//! - drag a note up/down: change its pitch (snaps to semitones; hold Shift for cents);
//! - drag a note's left or right edge: stretch it;
//! - double-click a note: split it there;
//! - arrow keys move the selected note; Ctrl+Z / Ctrl+Shift+Z undo and redo;
//! - mouse wheel scrolls pitch, Shift+wheel scrolls time, Ctrl+wheel zooms time.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Pos2, Rect, RichText, Sense, Shape,
    Stroke, StrokeKind, Ui, pos2,
};
use solplugs_pitch_core::scale::note_name;

use super::theme::{self, Palette};
use crate::engine::{Engine, Graph, GraphStatus};

const KEYS_W: f32 = 44.0;
const RULER_H: f32 = 20.0;
const EDGE_PX: f32 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Pitch {
        note: usize,
        start_y: f32,
        start_shift: f32,
    },
    Start {
        note: usize,
    },
    End {
        note: usize,
    },
}

/// View state for graph mode.
#[derive(Default)]
pub struct GraphView {
    /// Seconds per pixel's inverse: pixels per second.
    px_per_s: f32,
    /// Time at the left edge, seconds into the take.
    left_s: f32,
    /// Pitch at the top edge (MIDI).
    top: f32,
    /// Pixels per semitone.
    row: f32,
    /// Which take the view was fitted to (start, length).
    fitted: Option<(i64, usize)>,
    selected: Option<usize>,
    drag: Option<Drag>,
}

struct Geo {
    rect: Rect,
    px_per_s: f32,
    left_s: f32,
    top: f32,
    row: f32,
    sr: f32,
}

impl Geo {
    fn x(&self, t: i64) -> f32 {
        self.rect.left() + (t as f32 / self.sr - self.left_s) * self.px_per_s
    }
    fn t(&self, x: f32) -> i64 {
        (((x - self.rect.left()) / self.px_per_s + self.left_s) * self.sr) as i64
    }
    fn y(&self, m: f32) -> f32 {
        self.rect.top() + (self.top - m) * self.row
    }
    fn m(&self, y: f32) -> f32 {
        self.top - (y - self.rect.top()) / self.row
    }
}

pub fn show(ui: &mut Ui, engine: &Engine, v: &mut GraphView, pal: &Palette) {
    toolbar(ui, engine, v, pal);
    ui.add_space(6.0);
    theme::card(pal).show(ui, |ui| {
        let size = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        canvas(ui, engine, v, pal, rect, &resp);
    });
}

fn toolbar(ui: &mut Ui, engine: &Engine, v: &mut GraphView, pal: &Palette) {
    let (status, has_take, msg, can_undo, can_redo) = {
        let g = engine.graph();
        (
            g.status,
            g.take.is_some(),
            g.message.clone(),
            !g.undo.is_empty(),
            !g.redo.is_empty(),
        )
    };
    ui.horizontal(|ui| {
        let armed = engine.armed();
        let rec = egui::Button::new(
            RichText::new(if armed { "Capture armed" } else { "Capture" })
                .color(if armed { pal.bg } else { pal.record }),
        )
        .fill(if armed { pal.record } else { pal.panel_hi })
        .corner_radius(CornerRadius::same(12));
        if ui
            .add(rec)
            .on_hover_text("Arm, then play the track in your DAW: what plays through this plugin is recorded. Stop the DAW and the notes appear. Capturing again replaces the take.")
            .clicked()
        {
            engine.arm_capture(!armed);
        }
        let label = match status {
            GraphStatus::Empty => "No take yet: press Capture and play the track.",
            GraphStatus::Armed => "Armed: start playback in your DAW.",
            GraphStatus::Capturing => "Recording. Stop playback when done.",
            GraphStatus::Analysing => "Finding notes...",
            GraphStatus::Ready => "Drag notes to retune, drag edges to stretch, double-click to split.",
        };
        ui.label(RichText::new(label).color(pal.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(has_take, |ui| {
                if ui.button("Clear take").on_hover_text("Forget the take and its edits.").clicked() {
                    engine.clear_take();
                    v.selected = None;
                    v.fitted = None;
                }
                if ui.button("Reset").on_hover_text("Undo every edit: all notes back as sung.").clicked() {
                    let mut g = engine.graph();
                    g.checkpoint();
                    g.edits.reset();
                    g.touch();
                }
                if ui
                    .button("Snap to scale")
                    .on_hover_text("Moves every note's centre onto the nearest note of the key and scale.")
                    .clicked()
                {
                    let scale = engine.scale();
                    let mut g = engine.graph();
                    g.checkpoint();
                    g.edits.snap_all(&scale);
                    g.touch();
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                    engine.graph().redo();
                }
                if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                    engine.graph().undo();
                }
                if ui.button("Fit").on_hover_text("Show the whole take.").clicked() {
                    v.fitted = None;
                }
            });
        });
    });
    if let Some(m) = msg {
        ui.label(RichText::new(m).color(pal.record));
    }
}

fn fit(v: &mut GraphView, g: &Graph, width: f32, height: f32, sr: f32) {
    let len = g.take.as_ref().map_or(0, |t| t.len());
    v.px_per_s = ((width - KEYS_W) / (len as f32 / sr).max(1.0)).clamp(10.0, 2000.0);
    v.left_s = 0.0;
    let pitches: Vec<f32> = g.edits.notes.iter().map(|n| n.target()).collect();
    let (lo, hi) = if pitches.is_empty() {
        (55.0, 72.0)
    } else {
        (
            pitches.iter().copied().fold(f32::MAX, f32::min) - 4.0,
            pitches.iter().copied().fold(f32::MIN, f32::max) + 4.0,
        )
    };
    v.row = ((height - RULER_H) / (hi - lo).max(10.0)).clamp(8.0, 40.0);
    v.top = (hi + lo) / 2.0 + (height - RULER_H) / v.row / 2.0;
}

fn canvas(
    ui: &mut Ui,
    engine: &Engine,
    v: &mut GraphView,
    pal: &Palette,
    rect: Rect,
    resp: &egui::Response,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(4), pal.bg);
    let mut g = engine.graph();
    let Some(take) = g.take.clone() else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Press Capture, then play the vocal in your DAW.",
            FontId::proportional(16.0),
            pal.text_dim,
        );
        return;
    };
    let sr = take.sample_rate;
    let key = (take.start, take.len());
    if v.fitted != Some(key) || v.px_per_s <= 0.0 {
        fit(v, &g, rect.width(), rect.height(), sr);
        v.fitted = Some(key);
    }
    let roll = Rect::from_min_max(pos2(rect.left() + KEYS_W, rect.top() + RULER_H), rect.max);
    // Navigation.
    if resp.hovered() {
        let (scroll, zoom, hover) =
            ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
        if zoom != 1.0
            && let Some(h) = hover
        {
            let at = (h.x - roll.left()) / v.px_per_s + v.left_s;
            v.px_per_s = (v.px_per_s * zoom).clamp(5.0, 5000.0);
            v.left_s = at - (h.x - roll.left()) / v.px_per_s;
        }
        v.left_s -= scroll.x / v.px_per_s;
        v.top += scroll.y / v.row;
    }
    let total_s = take.len() as f32 / sr;
    v.left_s = v.left_s.clamp(-1.0, total_s.max(1.0));
    v.top = v.top.clamp(30.0, 110.0);
    let geo = Geo {
        rect: roll,
        px_per_s: v.px_per_s,
        left_s: v.left_s,
        top: v.top,
        row: v.row,
        sr,
    };
    draw_grid(&p, &geo, engine, pal, rect);

    // Hit testing.
    let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
    let hit = pointer.and_then(|pt| hit_note(&g, &geo, pt));
    if let Some((_, part)) = hit {
        let icon = match part {
            Part::Body => CursorIcon::Grab,
            Part::Left | Part::Right => CursorIcon::ResizeHorizontal,
        };
        ui.ctx().set_cursor_icon(if v.drag.is_some() {
            CursorIcon::Grabbing
        } else {
            icon
        });
    }
    interact(ui, engine, v, &mut g, &geo, resp, hit);

    // Notes.
    let an = g.analysis.clone();
    for (i, n) in g.edits.notes.iter().enumerate() {
        let (x0, x1) = (geo.x(n.dst_start), geo.x(n.dst_end));
        if x1 < roll.left() || x0 > roll.right() {
            continue;
        }
        let sel = v.selected == Some(i);
        if n.shift.abs() > 0.01 {
            // Where it was sung.
            let yo = geo.y(n.pitch);
            let r = Rect::from_min_max(pos2(x0, yo - v.row * 0.4), pos2(x1, yo + v.row * 0.4));
            p.rect_stroke(
                r,
                CornerRadius::same(6),
                Stroke::new(1.0, pal.note.gamma_multiply(0.5)),
                StrokeKind::Inside,
            );
        }
        let y = geo.y(n.target());
        let r = Rect::from_min_max(
            pos2(x0 + 1.0, y - v.row * 0.45),
            pos2(x1 - 1.0, y + v.row * 0.45),
        );
        let fill = if sel {
            pal.note
        } else {
            pal.note.gamma_multiply(0.7)
        };
        p.rect_filled(r, CornerRadius::same(6), fill);
        if sel {
            p.rect_stroke(
                r,
                CornerRadius::same(6),
                Stroke::new(1.5, pal.selected),
                StrokeKind::Outside,
            );
        }
        if r.width() > 34.0 {
            let cents = ((n.target() - n.target().round()) * 100.0).round();
            let label = if cents.abs() >= 1.0 {
                format!("{} {cents:+.0}", note_name(n.target().round() as i32))
            } else {
                note_name(n.target().round() as i32)
            };
            p.text(
                pos2(r.left() + 5.0, r.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                Color32::from_rgb(0x12, 0x10, 0x1C),
            );
        }
    }
    // Pitch curve as it will play.
    if let Some(an) = an {
        let mut run: Vec<Pos2> = Vec::new();
        let stroke = Stroke::new(1.5, pal.output);
        let step = ((1.5 / v.px_per_s * sr) as usize / an.hop).max(1);
        for (k, f) in an.frames.iter().enumerate().step_by(step) {
            let src = (k * an.hop) as f64;
            if f.midi <= 0.0 {
                if run.len() > 1 {
                    p.add(Shape::line(std::mem::take(&mut run), stroke));
                }
                run.clear();
                continue;
            }
            let dst = g.edits.map(src) as i64;
            let m = f.midi + g.edits.shift_at(src, sr);
            let pt = pos2(geo.x(dst), geo.y(m));
            if pt.x >= roll.left() - 2.0 && pt.x <= roll.right() + 2.0 {
                run.push(pt);
            }
        }
        if run.len() > 1 {
            p.add(Shape::line(run, stroke));
        }
    }
    drop(g);
    // Playhead.
    let (pos, playing) = engine.playhead();
    if playing {
        let t = pos - i64::from(engine.latency()) - take.start;
        let x = geo.x(t);
        if x >= roll.left() && x <= roll.right() {
            p.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.5, pal.text),
            );
        }
    }
}

fn draw_grid(p: &egui::Painter, geo: &Geo, engine: &Engine, pal: &Palette, full: Rect) {
    let roll = geo.rect;
    let scale = engine.scale();
    let lo = geo.m(roll.bottom()).floor() as i32;
    let hi = geo.m(roll.top()).ceil() as i32;
    for n in lo..=hi {
        let (y0, y1) = (geo.y(n as f32 + 0.5), geo.y(n as f32 - 0.5));
        let row = Rect::from_min_max(
            pos2(roll.left(), y0.max(roll.top())),
            pos2(roll.right(), y1.min(roll.bottom())),
        );
        if row.height() <= 0.0 {
            continue;
        }
        if scale.contains(n) {
            p.rect_filled(row, CornerRadius::ZERO, pal.row_in_scale);
        }
        p.line_segment(
            [pos2(roll.left(), y1), pos2(roll.right(), y1)],
            Stroke::new(0.5, pal.grid),
        );
        // Keyboard.
        let key = Rect::from_min_max(
            pos2(full.left(), row.top()),
            pos2(roll.left() - 2.0, row.bottom()),
        );
        let black = matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        p.rect_filled(
            key,
            CornerRadius::ZERO,
            if black { pal.bg } else { pal.panel_hi },
        );
        if !black && geo.row >= 10.0 {
            p.text(
                pos2(key.left() + 3.0, key.center().y),
                Align2::LEFT_CENTER,
                note_name(n),
                FontId::proportional(9.5_f32.min(geo.row - 1.0)),
                pal.text_dim,
            );
        }
    }
    // Ruler: a tick every 0.1/0.5/1/5/10 s depending on zoom.
    let step = [0.1f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]
        .into_iter()
        .find(|s| s * geo.px_per_s >= 70.0)
        .unwrap_or(60.0);
    let t0 = (geo.left_s / step).floor() * step;
    let mut t = t0;
    let right_s = geo.left_s + roll.width() / geo.px_per_s;
    while t <= right_s {
        let x = roll.left() + (t - geo.left_s) * geo.px_per_s;
        if x >= roll.left() {
            p.line_segment(
                [pos2(x, full.top() + RULER_H - 6.0), pos2(x, roll.bottom())],
                Stroke::new(0.5, pal.grid),
            );
            p.text(
                pos2(x + 3.0, full.top() + 2.0),
                Align2::LEFT_TOP,
                format!("{t:.2}s"),
                FontId::proportional(10.0),
                pal.text_dim,
            );
        }
        t += step;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Body,
    Left,
    Right,
}

fn hit_note(g: &Graph, geo: &Geo, pt: Pos2) -> Option<(usize, Part)> {
    if !geo.rect.contains(pt) {
        return None;
    }
    for (i, n) in g.edits.notes.iter().enumerate() {
        let (x0, x1) = (geo.x(n.dst_start), geo.x(n.dst_end));
        let y = geo.y(n.target());
        if pt.y < y - geo.row * 0.6
            || pt.y > y + geo.row * 0.6
            || pt.x < x0 - 2.0
            || pt.x > x1 + 2.0
        {
            continue;
        }
        let edge = EDGE_PX.min((x1 - x0) / 4.0);
        let part = if pt.x - x0 < edge {
            Part::Left
        } else if x1 - pt.x < edge {
            Part::Right
        } else {
            Part::Body
        };
        return Some((i, part));
    }
    None
}

fn interact(
    ui: &Ui,
    _engine: &Engine,
    v: &mut GraphView,
    g: &mut Graph,
    geo: &Geo,
    resp: &egui::Response,
    hit: Option<(usize, Part)>,
) {
    let sr = geo.sr;
    if resp.drag_started()
        && let Some((i, part)) = hit
    {
        {
            v.selected = Some(i);
            g.checkpoint();
            let start_y = resp.interact_pointer_pos().map_or(0.0, |p| p.y);
            v.drag = Some(match part {
                Part::Body => Drag::Pitch {
                    note: i,
                    start_y,
                    start_shift: g.edits.notes[i].shift,
                },
                Part::Left => Drag::Start { note: i },
                Part::Right => Drag::End { note: i },
            });
        }
    }
    if resp.dragged()
        && let (Some(d), Some(pt)) = (v.drag, resp.interact_pointer_pos())
    {
        match d {
            Drag::Pitch {
                note,
                start_y,
                start_shift,
            } => {
                let fine = ui.input(|i| i.modifiers.shift);
                if let Some(n) = g.edits.notes.get(note).copied() {
                    let raw = n.pitch + start_shift + (start_y - pt.y) / geo.row;
                    let target = if fine { raw } else { raw.round() };
                    g.edits.set_shift(note, target - n.pitch);
                }
            }
            Drag::Start { note } => {
                g.edits.set_start(note, geo.t(pt.x), sr);
            }
            Drag::End { note } => {
                g.edits.set_end(note, geo.t(pt.x), sr);
            }
        }
        g.touch();
    }
    if resp.drag_stopped() {
        v.drag = None;
    }
    if resp.double_clicked()
        && let (Some((i, _)), Some(pt), Some(an)) =
            (hit, resp.interact_pointer_pos(), g.analysis.clone())
    {
        g.checkpoint();
        if g.edits.split(i, geo.t(pt.x), &an) {
            g.touch();
        } else {
            g.undo.pop();
        }
    } else if resp.clicked() {
        v.selected = hit.map(|h| h.0);
    }
    // Keys.
    let (undo, redo, up, down, fine) = ui.input(|i| {
        let cmd = i.modifiers.command;
        (
            cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
            cmd && ((i.modifiers.shift && i.key_pressed(Key::Z)) || i.key_pressed(Key::Y)),
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.modifiers.shift,
        )
    });
    if undo {
        g.undo();
    }
    if redo {
        g.redo();
    }
    if let Some(i) = v.selected.filter(|i| *i < g.edits.notes.len())
        && (up || down)
    {
        let step = if fine { 0.1 } else { 1.0 };
        let n = g.edits.notes[i];
        g.checkpoint();
        let target = n.target() + if up { step } else { -step };
        let target = if fine { target } else { target.round() };
        g.edits.set_shift(i, target - n.pitch);
        g.touch();
    }
}
