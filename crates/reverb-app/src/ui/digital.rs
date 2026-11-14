//! The digital view: the big decay-curve window (lows, mids and highs, with the reverb
//! time readout), the BEFORE and AFTER waveforms stacked, the post meter (average peak
//! before and after) and MATCH. Laid out by hand so it squeezes cleanly when small.

use std::collections::VecDeque;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use solplugs_reverb_core::curve::{self, Band};
use solplugs_reverb_core::{Version, VersionSettings};

use super::theme::Palette;
use super::{HISTORY, UiLocal, db_text, match_row};
use crate::engine::{Column, Engine};

/// Draws the digital view into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, local: &UiLocal, pal: &Palette) {
    let p = ui.painter_at(area.expand(1.0));
    p.rect_filled(area, CornerRadius::same(10), pal.panel);
    p.rect_stroke(
        area,
        CornerRadius::same(10),
        Stroke::new(1.0, pal.grid),
        egui::StrokeKind::Inside,
    );
    let inner = area.shrink(10.0);
    let compact = inner.height() < 330.0;
    let gap = if compact { 6.0 } else { 10.0 };
    // Decay window gets ~58% of the height; the waveform/meter row the rest.
    let curve_h = (inner.height() * 0.58).max(90.0);
    let curve_r = Rect::from_min_size(inner.min, vec2(inner.width(), curve_h));
    let row = Rect::from_min_max(pos2(inner.left(), curve_r.bottom() + gap), inner.max);
    let v = engine.version();
    let s = engine.params().version_settings(v);
    decay_window(&p, curve_r, v, &s, engine.sample_rate(), pal, compact);

    let side_w = (row.width() * 0.27).clamp(150.0, 300.0);
    let waves = Rect::from_min_max(row.min, pos2(row.right() - side_w - gap, row.bottom()));
    let side = Rect::from_min_max(pos2(waves.right() + gap, row.top()), row.max);
    let wh = (waves.height() - 4.0) / 2.0;
    waveform(
        &p,
        Rect::from_min_size(waves.min, vec2(waves.width(), wh)),
        &local.history,
        false,
        engine,
        pal,
    );
    waveform(
        &p,
        Rect::from_min_size(
            pos2(waves.left(), waves.top() + wh + 4.0),
            vec2(waves.width(), wh),
        ),
        &local.history,
        true,
        engine,
        pal,
    );
    let match_h = (side.height() * 0.3).clamp(26.0, 48.0);
    let meter = Rect::from_min_max(side.min, pos2(side.right(), side.bottom() - match_h - 6.0));
    post_meter(&p, meter, engine, pal);
    match_row(
        ui,
        &p,
        Rect::from_min_max(pos2(side.left(), side.bottom() - match_h), side.max),
        engine,
        pal,
    );
}

/// The decay-curve window.
fn decay_window(
    p: &egui::Painter,
    r: Rect,
    v: Version,
    s: &VersionSettings,
    sr: f32,
    pal: &Palette,
    compact: bool,
) {
    p.rect_filled(r, CornerRadius::same(8), pal.card);
    let title = if compact { 12.0 } else { 15.0 };
    p.text(
        r.left_top() + vec2(12.0, 8.0),
        Align2::LEFT_TOP,
        "DECAY CURVE",
        FontId::proportional(title),
        pal.accent,
    );
    // Reverb time readout, top right: big seconds, small "RT60".
    let rt = curve::rt60(v, s, Band::Mid, sr);
    let big = if s.freeze {
        "FREEZE".to_owned()
    } else if rt < 10.0 {
        format!("{rt:.2} s")
    } else {
        format!("{rt:.1} s")
    };
    let g = p.text(
        r.right_top() + vec2(-12.0, 4.0),
        Align2::RIGHT_TOP,
        big,
        FontId::proportional(if compact { 18.0 } else { 26.0 }),
        if s.freeze { pal.sun } else { pal.text },
    );
    p.text(
        pos2(g.left() - 8.0, g.center().y),
        Align2::RIGHT_CENTER,
        "REVERB TIME",
        FontId::proportional(11.0),
        pal.dim,
    );
    // Legend.
    let lf = FontId::proportional(11.0);
    let mut x = r.left() + 12.0 + if compact { 100.0 } else { 130.0 };
    for (name, c) in [
        ("LOWS", lows_colour(pal)),
        ("MIDS", pal.accent),
        ("HIGHS", highs_colour(pal)),
    ] {
        p.line_segment(
            [pos2(x, r.top() + 16.0), pos2(x + 14.0, r.top() + 16.0)],
            Stroke::new(2.5, c),
        );
        let t = p.text(
            pos2(x + 18.0, r.top() + 16.0),
            Align2::LEFT_CENTER,
            name,
            lf.clone(),
            pal.dim,
        );
        x = t.right() + 12.0;
    }

    let left = if compact { 34.0 } else { 46.0 };
    let plot = Rect::from_min_max(
        pos2(r.left() + left, r.top() + if compact { 30.0 } else { 40.0 }),
        pos2(
            r.right() - 14.0,
            r.bottom() - if compact { 18.0 } else { 26.0 },
        ),
    );
    // Time span: enough to show the whole fall to -60 dB (frozen: a fixed 4 s).
    let total_ms = if s.freeze {
        4000.0
    } else {
        s.predelay_ms + curve::rt60(v, s, Band::Low, sr).max(rt) * 1000.0 * 1.15
    };
    let span = nice_span(total_ms.max(200.0));
    let x_of = |t: f32| plot.left() + plot.width() * (t / span).clamp(0.0, 1.0);
    let y_of = |db: f32| plot.top() + plot.height() * (-db / 66.0).clamp(0.0, 1.0);
    let font = FontId::proportional(if compact { 9.5 } else { 11.0 });
    for db in [0.0f32, -10.0, -20.0, -30.0, -40.0, -50.0, -60.0] {
        let y = y_of(db);
        p.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, pal.grid),
        );
        if !compact || (db as i32) % 20 == 0 {
            p.text(
                pos2(plot.left() - 5.0, y),
                Align2::RIGHT_CENTER,
                format!("{db:.0}"),
                font.clone(),
                pal.dim,
            );
        }
    }
    let step = span / 5.0;
    for k in 0..=5 {
        let t = step * k as f32;
        let x = x_of(t);
        p.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, pal.grid),
        );
        let label = if span >= 2000.0 {
            format!("{:.1} s", t / 1000.0)
        } else {
            format!("{t:.0} ms")
        };
        p.text(
            pos2(x, plot.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            font.clone(),
            pal.dim,
        );
    }
    if !compact {
        p.text(
            pos2(plot.left() - 40.0, plot.center().y),
            Align2::LEFT_CENTER,
            "dB",
            font.clone(),
            pal.dim,
        );
    }
    // Pre-delay marker.
    if s.predelay_ms > 0.5 {
        let x = x_of(s.predelay_ms);
        p.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, pal.dim.gamma_multiply(0.5)),
        );
        p.text(
            pos2(x + 4.0, plot.bottom() - 2.0),
            Align2::LEFT_BOTTOM,
            format!("pre-delay {:.0} ms", s.predelay_ms),
            font.clone(),
            pal.dim,
        );
    }
    let n = (plot.width() as usize).clamp(60, 600);
    let points = |band: Band| -> Vec<Pos2> {
        (0..=n)
            .map(|i| {
                let t = span * i as f32 / n as f32;
                pos2(x_of(t), y_of(curve::level_db(v, s, band, t, sr)))
            })
            .collect()
    };
    let mids = points(Band::Mid);
    // Fill under the mids.
    let fill = pal.accent.gamma_multiply(0.12);
    for w in mids.windows(2) {
        let quad = vec![
            w[0],
            w[1],
            pos2(w[1].x, plot.bottom()),
            pos2(w[0].x, plot.bottom()),
        ];
        p.add(Shape::convex_polygon(quad, fill, Stroke::NONE));
    }
    p.add(Shape::line(
        points(Band::Low),
        Stroke::new(1.5, lows_colour(pal)),
    ));
    p.add(Shape::line(
        points(Band::High),
        Stroke::new(1.5, highs_colour(pal)),
    ));
    p.add(Shape::line(mids, Stroke::new(2.5, pal.accent)));
}

fn lows_colour(pal: &Palette) -> Color32 {
    pal.sun.gamma_multiply(0.85)
}

fn highs_colour(_pal: &Palette) -> Color32 {
    Color32::from_rgb(0x8F, 0xA8, 0xFF)
}

/// A round time span, ms.
fn nice_span(ms: f32) -> f32 {
    for s in [
        250.0, 500.0, 1000.0, 1500.0, 2000.0, 3000.0, 5000.0, 7500.0, 10_000.0, 15_000.0, 20_000.0,
        30_000.0,
    ] {
        if ms <= s {
            return s;
        }
    }
    30_000.0
}

/// Columns to pixels: the newest column sits at the right edge.
fn column_at(history: &VecDeque<Column>, x: f32, width: f32) -> Option<&Column> {
    let n = history.len();
    if n == 0 {
        return None;
    }
    let back = ((1.0 - x / width) * HISTORY as f32) as usize;
    n.checked_sub(back + 1).and_then(|i| history.get(i))
}

/// A waveform strip (BEFORE or AFTER), both channels together, with its level badge.
fn waveform(
    p: &egui::Painter,
    r: Rect,
    history: &VecDeque<Column>,
    after: bool,
    engine: &Engine,
    pal: &Palette,
) {
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
            let cols = if after { c.after } else { c.before };
            let lo = cols[0].0.min(cols[1].0);
            let hi = cols[0].1.max(cols[1].1);
            if lo <= hi {
                let y0 = mid - hi.clamp(-1.0, 1.0) * half;
                let y1 = mid - lo.clamp(-1.0, 1.0) * half;
                p.line_segment(
                    [pos2(r.left() + x, y0), pos2(r.left() + x, y1.max(y0 + 1.0))],
                    Stroke::new(1.0, colour),
                );
            }
        }
        x += 1.0;
    }
    let small = r.height() < 40.0;
    let font = FontId::proportional(if small { 10.5 } else { 13.0 });
    let t = p.text(
        r.left_top() + vec2(8.0, 4.0),
        Align2::LEFT_TOP,
        title,
        font.clone(),
        colour,
    );
    let m = engine.meters();
    let lvl = if after { m.avg_after() } else { m.avg_before() };
    let badge = p.layout_no_wrap(
        db_text(lvl),
        FontId::proportional(if small { 10.0 } else { 12.0 }),
        pal.text,
    );
    let br = Rect::from_min_size(
        pos2(t.right() + 8.0, t.top() - 1.0),
        badge.size() + vec2(10.0, 2.0),
    );
    p.rect_filled(br, CornerRadius::same(8), pal.raised);
    p.galley(br.min + vec2(5.0, 1.0), badge, pal.text);
}

/// Bar position of a level, 0..1 over -60..0 dBFS.
fn bar_pos(db: f32) -> f32 {
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// The post meter: average peak before and after, side by side as vertical bars with
/// readouts.
fn post_meter(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let m = engine.meters();
    p.rect_filled(r, CornerRadius::same(6), pal.card);
    let font = FontId::proportional(if r.height() < 90.0 { 10.0 } else { 12.0 });
    let half = (r.width() - 12.0) / 2.0;
    for (i, (label, v, c)) in [
        ("BEFORE", m.avg_before(), pal.before),
        ("AFTER", m.avg_after(), pal.after),
    ]
    .into_iter()
    .enumerate()
    {
        let col = Rect::from_min_size(
            pos2(r.left() + 4.0 + i as f32 * (half + 4.0), r.top() + 4.0),
            vec2(half, r.height() - 8.0),
        );
        p.text(col.center_top(), Align2::CENTER_TOP, label, font.clone(), c);
        let val = p.text(
            col.center_bottom(),
            Align2::CENTER_BOTTOM,
            db_text(v),
            FontId::monospace(font.size),
            pal.text,
        );
        let bar_w = (col.width() * 0.28).clamp(8.0, 22.0);
        let bar = Rect::from_min_max(
            pos2(col.center().x - bar_w / 2.0, col.top() + font.size + 6.0),
            pos2(col.center().x + bar_w / 2.0, val.top() - 4.0),
        );
        if bar.height() > 6.0 {
            p.rect_filled(bar, CornerRadius::same(3), pal.bg);
            let fill = Rect::from_min_max(
                pos2(bar.left(), bar.bottom() - bar.height() * bar_pos(v)),
                bar.max,
            );
            p.rect_filled(fill, CornerRadius::same(3), c);
            for db in [-6.0f32, -12.0, -24.0, -48.0] {
                let y = bar.bottom() - bar.height() * bar_pos(db);
                p.line_segment(
                    [pos2(bar.right() + 2.0, y), pos2(bar.right() + 6.0, y)],
                    Stroke::new(1.0, pal.dim),
                );
            }
        }
    }
}
