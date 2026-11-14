//! The flat digital side view: BEFORE and AFTER waveforms stacked on one scale, the
//! scrolling gain-reduction trace, digital readouts, the post meter (average peak before
//! and after) and the MATCH button. Laid out by hand so it squeezes cleanly when the
//! window is small.

use std::collections::VecDeque;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use super::theme::Palette;
use crate::engine::{Column, Engine};
use crate::params::{self, K, Pid};

/// Below this height the view drops its captions and shrinks the tiles.
const COMPACT: f32 = 560.0;

/// Fixed heights of the non-graph rows.
struct Rows {
    head: f32,
    readouts: f32,
    mix: f32,
    post: f32,
    matchb: f32,
    caption: f32,
    gap: f32,
}

impl Rows {
    fn for_height(h: f32) -> Self {
        if h < COMPACT {
            Self {
                head: 24.0,
                readouts: 40.0,
                mix: 24.0,
                post: 44.0,
                matchb: 34.0,
                caption: 0.0,
                gap: 5.0,
            }
        } else {
            Self {
                head: 30.0,
                readouts: 52.0,
                mix: 28.0,
                post: 66.0,
                matchb: 46.0,
                caption: 18.0,
                gap: 8.0,
            }
        }
    }

    fn fixed(&self) -> f32 {
        self.head
            + self.readouts
            + self.mix
            + self.post
            + self.matchb
            + self.caption
            + self.gap * 8.0
    }
}

/// Cuts rows off the top of a rect.
struct Cursor {
    rect: Rect,
    y: f32,
    gap: f32,
}

impl Cursor {
    fn take(&mut self, h: f32) -> Rect {
        let r = Rect::from_min_size(pos2(self.rect.left(), self.y), vec2(self.rect.width(), h));
        self.y += h + self.gap;
        r
    }
}

/// Draws the side view into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, history: &VecDeque<Column>, pal: &Palette) {
    let painter = ui.painter_at(area.expand(1.0));
    painter.rect_filled(area, CornerRadius::same(10), pal.panel);
    let inner = area.shrink(12.0);
    let rows = Rows::for_height(inner.height());
    let graphs = (inner.height() - rows.fixed()).max(90.0);
    let mut cur = Cursor {
        rect: inner,
        y: inner.top(),
        gap: rows.gap,
    };

    header(&painter, cur.take(rows.head), engine, pal);
    waveform(
        &painter,
        cur.take(graphs * 0.36),
        history,
        |c| c.before,
        "BEFORE",
        pal.before,
        pal,
    );
    waveform(
        &painter,
        cur.take(graphs * 0.36),
        history,
        |c| c.after,
        "AFTER",
        pal.after,
        pal,
    );
    gr_trace(&painter, cur.take(graphs * 0.28), history, pal);
    readouts(&painter, cur.take(rows.readouts), engine, pal);
    mix_slider(ui, &painter, cur.take(rows.mix), engine, pal);
    post_meter(&painter, cur.take(rows.post), engine, pal);
    match_row(ui, &painter, cur.take(rows.matchb), engine, pal);
    if rows.caption > 0.0 {
        let r = cur.take(rows.caption);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            "MATCH sets Gain Out once so AFTER is as loud as BEFORE; you compare compression, not volume.",
            FontId::proportional(11.5),
            pal.dim,
        );
    }
}

fn header(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let mode = engine.mode();
    let font = FontId::proportional((r.height() * 0.6).max(13.0));
    let label = p.text(
        r.left_center(),
        Align2::LEFT_CENTER,
        "MODE",
        font.clone(),
        pal.dim,
    );
    let galley = p.layout_no_wrap(mode.name().to_owned(), font, pal.text);
    let chip = Rect::from_min_size(
        pos2(
            label.right() + 8.0,
            r.center().y - galley.size().y / 2.0 - 2.0,
        ),
        galley.size() + vec2(16.0, 4.0),
    );
    p.rect_filled(chip, CornerRadius::same(6), pal.before.gamma_multiply(0.35));
    p.galley(chip.min + vec2(8.0, 2.0), galley, pal.text);
    let bypass = engine.params().on(Pid::g(crate::params::G::Bypass));
    let status = if bypass { "BYPASSED" } else { "ACTIVE" };
    p.text(
        r.right_center(),
        Align2::RIGHT_CENTER,
        status,
        FontId::proportional(12.0),
        if bypass { pal.warn } else { pal.good },
    );
}

/// Columns to pixels: the newest column sits at the right edge.
fn column_at(history: &VecDeque<Column>, x: f32, width: f32) -> Option<&Column> {
    let n = history.len();
    if n == 0 {
        return None;
    }
    let span = super::HISTORY as f32;
    let back = ((1.0 - x / width) * span) as usize;
    n.checked_sub(back + 1).and_then(|i| history.get(i))
}

fn waveform(
    p: &egui::Painter,
    r: Rect,
    history: &VecDeque<Column>,
    pick: impl Fn(&Column) -> (f32, f32),
    title: &str,
    colour: Color32,
    pal: &Palette,
) {
    p.rect_filled(r, CornerRadius::same(6), pal.card);
    let mid = r.center().y;
    let half = r.height() * 0.45;
    p.line_segment(
        [pos2(r.left(), mid), pos2(r.right(), mid)],
        Stroke::new(1.0, pal.grid),
    );
    let w = r.width().max(1.0);
    let mut x = 0.0;
    while x < w {
        if let Some(c) = column_at(history, x, w) {
            let (lo, hi) = pick(c);
            let y0 = mid - hi.clamp(-1.0, 1.0) * half;
            let y1 = mid - lo.clamp(-1.0, 1.0) * half;
            p.line_segment(
                [pos2(r.left() + x, y0), pos2(r.left() + x, y1.max(y0 + 1.0))],
                Stroke::new(1.0, colour),
            );
        }
        x += 1.0;
    }
    let tag = p.text(
        r.left_top() + vec2(8.0, 5.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        colour,
    );
    p.circle_filled(pos2(tag.right() + 7.0, tag.center().y), 3.0, colour);
    p.text(
        r.right_top() + vec2(-8.0, 5.0),
        Align2::RIGHT_TOP,
        if title == "BEFORE" {
            "dry input"
        } else {
            "wet output"
        },
        FontId::proportional(11.0),
        pal.dim,
    );
}

/// GR display range: the next step that holds the recent peak.
fn gr_range(history: &VecDeque<Column>) -> f32 {
    let peak = history.iter().map(|c| c.gr).fold(0.0f32, f32::max);
    [6.0, 12.0, 24.0, 40.0]
        .into_iter()
        .find(|s| peak <= *s)
        .unwrap_or(40.0)
}

fn gr_trace(p: &egui::Painter, r: Rect, history: &VecDeque<Column>, pal: &Palette) {
    p.rect_filled(r, CornerRadius::same(6), pal.card);
    let range = gr_range(history);
    let y_of = |db: f32| r.top() + (db / range).clamp(0.0, 1.0) * (r.height() - 4.0) + 2.0;
    for step in [0.25, 0.5, 0.75] {
        let y = y_of(range * step);
        p.line_segment(
            [pos2(r.left(), y), pos2(r.right(), y)],
            Stroke::new(1.0, pal.grid),
        );
        p.text(
            pos2(r.right() - 6.0, y),
            Align2::RIGHT_BOTTOM,
            format!("-{:.0}", range * step),
            FontId::proportional(10.0),
            pal.dim,
        );
    }
    let w = r.width().max(1.0);
    let mut pts: Vec<Pos2> = Vec::with_capacity(w as usize);
    let mut x = 0.0;
    while x < w {
        if let Some(c) = column_at(history, x, w) {
            pts.push(pos2(r.left() + x, y_of(c.gr)));
        }
        x += 1.0;
    }
    for pair in pts.windows(2) {
        let fill = Rect::from_min_max(pos2(pair[0].x, r.top() + 2.0), pos2(pair[1].x, pair[0].y));
        p.rect_filled(fill, CornerRadius::ZERO, pal.gr.gamma_multiply(0.18));
    }
    if pts.len() > 1 {
        p.add(Shape::line(pts, Stroke::new(1.6, pal.gr)));
    }
    p.text(
        r.left_bottom() + vec2(8.0, -5.0),
        Align2::LEFT_BOTTOM,
        "GAIN REDUCTION",
        FontId::proportional(12.0),
        pal.gr,
    );
}

fn db_text(v: f32) -> String {
    if v <= -90.0 {
        "-inf".to_owned()
    } else {
        format!("{v:.1}")
    }
}

fn tile(p: &egui::Painter, r: Rect, label: &str, value: &str, colour: Color32, pal: &Palette) {
    p.rect_filled(r, CornerRadius::same(6), pal.card);
    let small = r.height() < 46.0;
    p.text(
        r.center_top() + vec2(0.0, 4.0),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(if small { 10.0 } else { 11.0 }),
        pal.dim,
    );
    let size = (r.height() * 0.42).clamp(13.0, 30.0).min(r.width() / 4.2);
    p.text(
        r.center_bottom() - vec2(0.0, 4.0),
        Align2::CENTER_BOTTOM,
        value,
        FontId::monospace(size.max(11.0)),
        colour,
    );
}

fn split_h(r: Rect, n: usize, gap: f32) -> Vec<Rect> {
    let w = (r.width() - gap * (n as f32 - 1.0)) / n as f32;
    (0..n)
        .map(|i| {
            Rect::from_min_size(
                pos2(r.left() + i as f32 * (w + gap), r.top()),
                vec2(w, r.height()),
            )
        })
        .collect()
}

fn readouts(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let m = engine.meters();
    let (l, rr) = m.gr();
    let gr = l.max(rr);
    let cells = split_h(r, 5, 6.0);
    let items = [
        ("GR dB", format!("{:.1}", -gr.max(0.0)), pal.gr),
        ("IN dB", db_text(m.input()), pal.before),
        ("OUT dB", db_text(m.output()), pal.after),
        ("WET dB", db_text(m.wet()), pal.text),
        ("DRY dB", db_text(m.dry()), pal.text),
    ];
    for (cell, (label, value, colour)) in cells.into_iter().zip(items) {
        tile(p, cell, label, &value, colour, pal);
    }
}

fn mix_slider(ui: &mut Ui, p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let id = Pid::m(engine.mode(), K::Mix);
    let Some(info) = id.info() else {
        return;
    };
    let v = engine.params().get(id);
    let font = FontId::proportional(12.0);
    p.text(
        r.left_center(),
        Align2::LEFT_CENTER,
        format!("MIX {:.0}%", v),
        font.clone(),
        pal.dim,
    );
    p.text(r.right_center(), Align2::RIGHT_CENTER, "WET", font, pal.dim);
    let track = Rect::from_min_max(
        pos2(r.left() + 76.0, r.top()),
        pos2(r.right() - 40.0, r.bottom()),
    );
    let resp = ui
        .interact(
            track,
            ui.scope_id().with("comp-mix-slider"),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    if let Some(pos) = resp.interact_pointer_pos() {
        let n = f64::from((pos.x - track.left()) / track.width().max(1.0));
        if resp.drag_started() {
            engine.begin_edit(id);
        }
        if resp.dragged() || resp.drag_started() {
            engine.edit(id, info.denormalize(n));
        } else if resp.clicked() {
            engine.set(id, info.denormalize(n));
        }
    }
    if resp.drag_stopped() {
        engine.end_edit(id);
    }
    if resp.double_clicked() {
        engine.set(id, info.default);
    }
    let norm = info.normalize(engine.params().get(id)) as f32;
    let bar = Rect::from_center_size(track.center(), vec2(track.width(), 6.0));
    p.rect_filled(bar, CornerRadius::same(3), pal.raised);
    let mut fill = bar;
    fill.set_width(bar.width() * norm);
    p.rect_filled(fill, CornerRadius::same(3), pal.after.gamma_multiply(0.8));
    let x = track.left() + track.width() * norm;
    p.circle_filled(
        pos2(x, track.center().y),
        8.0,
        if resp.hovered() { pal.text } else { pal.dim },
    );
    if resp.hovered() || resp.dragged() {
        let at = pos2(
            x.clamp(track.left() + 30.0, track.right() - 30.0),
            track.top() - 2.0,
        );
        let g = p.layout_no_wrap(
            format!("Mix {}", params::format(id, v)),
            FontId::proportional(11.5),
            pal.bg,
        );
        let pill = Rect::from_center_size(
            at - vec2(0.0, g.size().y / 2.0 + 2.0),
            g.size() + vec2(10.0, 4.0),
        );
        p.rect_filled(pill, CornerRadius::same(4), pal.text);
        p.galley(pill.min + vec2(5.0, 2.0), g, pal.bg);
    }
    let _ = resp.on_hover_text(
        "Mix: blend of compressed (wet) and untouched (dry) signal, for parallel compression. Wet and Dry levels are on the faceplate.",
    );
}

fn post_meter(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let m = engine.meters();
    let cells = split_h(r, 2, 8.0);
    let fmt = |v: f32| {
        if v <= -90.0 {
            "-inf dB".to_owned()
        } else {
            format!("{v:.1} dB")
        }
    };
    tile(
        p,
        cells[0],
        "AVG PEAK BEFORE",
        &fmt(m.avg_before()),
        pal.before,
        pal,
    );
    tile(
        p,
        cells[1],
        "AVG PEAK AFTER",
        &fmt(m.avg_after()),
        pal.after,
        pal,
    );
    for c in &cells {
        p.rect_stroke(
            *c,
            CornerRadius::same(6),
            Stroke::new(1.0, pal.grid),
            StrokeKind::Inside,
        );
    }
}

fn match_row(ui: &mut Ui, p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let mode = engine.mode();
    let auto = engine.params().m(mode, K::AutoMakeup) >= 0.5;
    let toggle_w = (r.width() * 0.3).clamp(90.0, 150.0);
    let button = Rect::from_min_max(r.min, pos2(r.right() - toggle_w - 8.0, r.bottom()));
    let toggle = Rect::from_min_max(pos2(button.right() + 8.0, r.top()), r.max);

    let resp = ui
        .interact(button, ui.scope_id().with("comp-match"), Sense::click())
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
    p.rect_stroke(
        button,
        CornerRadius::same(10),
        Stroke::new(2.0, pal.before),
        StrokeKind::Inside,
    );
    p.text(
        button.center(),
        Align2::CENTER_CENTER,
        "MATCH",
        FontId::proportional((r.height() * 0.5).clamp(15.0, 26.0)),
        Color32::WHITE,
    );
    if resp
        .on_hover_text("Sets Gain Out so the AFTER average peak equals the BEFORE average peak. Play some audio first.")
        .clicked()
    {
        engine.match_now();
    }

    let t = ui
        .interact(
            toggle,
            ui.scope_id().with("comp-auto-makeup"),
            Sense::click(),
        )
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
    let line2 = if auto {
        format!("{:+.1} dB", engine.meters().auto_makeup())
    } else {
        "off".to_owned()
    };
    p.text(
        toggle.center() - vec2(0.0, 1.0),
        Align2::CENTER_BOTTOM,
        "AUTO MAKEUP",
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
    if t.on_hover_text("Auto makeup tracks Gain Out live so the output stays as loud as the input. MATCH is the press-once version.")
        .clicked()
    {
        engine.set_auto_makeup(!auto);
    }
}
