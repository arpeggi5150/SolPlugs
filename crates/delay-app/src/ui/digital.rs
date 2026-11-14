//! The digital view: the BEFORE and AFTER waveforms stacked (left and right shown
//! separately, each with its L/R meter), the post meter and MATCH, the time section (one big
//! TIME knob, the BPM display with the ms and the note value under it, SYNC and TAP TEMPO)
//! and the repeats picture with the feedback meter. Laid out by hand so it squeezes cleanly
//! when small.

use std::collections::VecDeque;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use solplugs_delay_core::curve::{self, Repeat};
use solplugs_delay_core::{Ctl, NOTES, Version, VersionSettings, note_ms, note_words};

use super::plate::{self, KnobLook, KnobOpts, Plate};
use super::theme::Palette;
use super::{HISTORY, UiLocal, db_text, match_row};
use crate::engine::{Column, Engine};
use crate::params::{G, Pid};

/// Time section design canvas.
const TIME_W: f32 = 640.0;
/// Time section design height.
const TIME_H: f32 = 190.0;

/// Draws the digital view into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, local: &UiLocal, pal: &Palette) {
    let p = ui.painter_at(area.expand(1.0));
    p.rect_filled(area, CornerRadius::same(10), pal.panel);
    p.rect_stroke(
        area,
        CornerRadius::same(10),
        Stroke::new(1.0, pal.grid),
        StrokeKind::Inside,
    );
    let inner = area.shrink(10.0);
    let compact = inner.height() < 330.0;
    let gap = if compact { 6.0 } else { 10.0 };
    // Waveforms get ~46% of the height; the time section and repeats the rest.
    let wave_h = (inner.height() * 0.46).max(80.0);
    let top = Rect::from_min_size(inner.min, vec2(inner.width(), wave_h));
    let bottom = Rect::from_min_max(pos2(inner.left(), top.bottom() + gap), inner.max);

    let side_w = (top.width() * 0.24).clamp(140.0, 280.0);
    let waves = Rect::from_min_max(top.min, pos2(top.right() - side_w - gap, top.bottom()));
    let side = Rect::from_min_max(pos2(waves.right() + gap, top.top()), top.max);
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

    let v = engine.version();
    let s = engine.params().version_settings(v);
    let time_w = (bottom.width() * 0.56).min(bottom.height() * TIME_W / TIME_H * 1.15);
    let time_r = Rect::from_min_size(bottom.min, vec2(time_w, bottom.height()));
    let rep_r = Rect::from_min_max(pos2(time_r.right() + gap, bottom.top()), bottom.max);
    repeats_window(&p, rep_r, engine, v, &s, pal, compact);
    time_section(ui, time_r, engine, v, &s, pal);
}

/// The time section: TIME knob, BPM display, SYNC and TAP TEMPO.
fn time_section(
    ui: &mut Ui,
    area: Rect,
    engine: &Engine,
    v: Version,
    s: &VersionSettings,
    pal: &Palette,
) {
    let p = ui.painter_at(area.expand(1.0));
    p.rect_filled(area, CornerRadius::same(8), pal.card);
    let mut pl = Plate::new(ui, area.shrink(4.0), TIME_W, TIME_H);
    pl.text(TIME_W / 2.0, 16.0, "TIME", 13.0, pal.dim);
    let look = KnobLook {
        body: Color32::from_rgb(0x15, 0x16, 0x1A),
        cap: Color32::from_rgb(0x26, 0x28, 0x2E),
        shine: Color32::from_white_alpha(24),
        pointer: Color32::WHITE,
        ink: pal.dim,
        ridges: 0,
        ticks: true,
    };
    // One big TIME knob: the note value when synced, the milliseconds when free.
    let id = if s.sync {
        Pid::c(v, Ctl::Note)
    } else {
        Pid::c(v, Ctl::Time)
    };
    plate::knob(
        &mut pl,
        engine,
        id,
        (78.0, 88.0, 40.0),
        &look,
        KnobOpts {
            label: "TIME",
            no_value: true,
            help: if s.sync {
                "Synced: turn for the note value (1/16 to 1/1, dotted and triplet)."
            } else {
                "Free: turn for the time in milliseconds. Tap tempo sets it by hand."
            },
            ..Default::default()
        },
    );

    // The display: BPM biggest, the ms and the note value smaller below.
    let m = engine.meters();
    let host = m.host_tempo();
    let bpm = if host {
        m.bpm()
    } else {
        engine.params().get(Pid::g(G::Tempo)) as f32
    };
    let ms = s.delay_ms(bpm);
    let d = pl.r(150.0, 34.0, 300.0, 116.0);
    let pp = &pl.painter;
    pp.rect_filled(
        d,
        CornerRadius::same(pl.s(10.0) as u8),
        Color32::from_rgb(0x0B, 0x12, 0x14),
    );
    pp.rect_stroke(
        d,
        CornerRadius::same(pl.s(10.0) as u8),
        Stroke::new(1.0, pal.grid),
        StrokeKind::Inside,
    );
    let big = pl.font(52.0);
    let unit = pl.font(24.0);
    let num = pp.layout_no_wrap(format!("{bpm:.0}"), big, pal.text);
    let bpm_txt = pp.layout_no_wrap(" BPM".into(), unit, pal.text);
    let total = num.size().x + bpm_txt.size().x;
    let x0 = d.center().x - total / 2.0;
    let base = pl.p(0.0, 92.0).y;
    let num_w = num.size().x;
    pp.galley(pos2(x0, base - num.size().y), num, pal.text);
    pp.galley(
        pos2(x0 + num_w, base - bpm_txt.size().y - pl.s(5.0)),
        bpm_txt,
        pal.text,
    );
    let note = if s.sync {
        note_words(s.note)
    } else {
        nearest_note(ms, bpm).map_or("free".to_owned(), |n| format!("≈ {}", note_words(n)))
    };
    pl.text(
        300.0,
        116.0,
        &format!("{} • {note}", ms_label(ms)),
        17.0,
        pal.before,
    );
    pl.text(
        300.0,
        138.0,
        if host {
            "song tempo"
        } else {
            "own tempo · drag to set"
        },
        10.0,
        pal.dim,
    );
    // Without a host tempo, drag the display to set the delay's own tempo.
    if !host {
        let tempo = Pid::g(G::Tempo);
        let resp = pl
            .ui
            .interact(d, pl.ui.scope_id().with("dl-tempo"), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::ResizeVertical)
            .on_hover_text("No song tempo from the host: drag up/down to set the delay's own tempo, double-click for 120 BPM.");
        if let Some(info) = tempo.info() {
            plate::drag_logic(
                pl.ui,
                engine,
                tempo,
                &resp,
                info.normalize(f64::from(bpm)),
                false,
            );
        }
    }

    // SYNC toggle and TAP TEMPO beside the display.
    let sync_r = pl.r(470.0, 44.0, 150.0, 40.0);
    let sync_id = Pid::c(v, Ctl::Sync);
    let resp = pl
        .ui
        .interact(sync_r, pl.ui.scope_id().with("dl-sync"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(sync_id.info().map_or("", |i| i.help));
    if resp.clicked() {
        engine.set(sync_id, if s.sync { 0.0 } else { 1.0 });
    }
    let pp = &pl.painter;
    pp.rect_filled(sync_r, CornerRadius::same(pl.s(10.0) as u8), pal.raised);
    pl.text_at(
        482.0,
        64.0,
        Align2::LEFT_CENTER,
        "SYNC",
        pl.font(14.0),
        pal.text,
    );
    let pill = pl.r(548.0, 52.0, 60.0, 24.0);
    pp.rect_filled(
        pill,
        CornerRadius::same(pl.s(12.0) as u8),
        if s.sync { pal.before } else { pal.card },
    );
    let knob_x = if s.sync { 596.0 } else { 560.0 };
    pp.circle_filled(pl.p(knob_x, 64.0), pl.s(10.0), pal.text);

    let tap_r = pl.r(470.0, 96.0, 150.0, 44.0);
    let tap = plate::button(
        &mut pl,
        ("dl-tap", 0),
        tap_r,
        "TAP TEMPO",
        false,
        pal.raised,
        pal.accent,
        pal.text,
    );
    if tap
        .on_hover_text(
            "Tap at least twice: the time follows the gaps between your taps (sync turns off).",
        )
        .clicked()
    {
        engine.tap();
    }
    pl.text(
        545.0,
        156.0,
        if s.sync {
            NOTES.get(s.note).copied().unwrap_or("")
        } else {
            "free time"
        },
        10.0,
        pal.dim,
    );
}

/// The nearest note value to `ms` at `bpm`, if within 2 %.
fn nearest_note(ms: f32, bpm: f32) -> Option<usize> {
    (0..NOTES.len())
        .map(|i| (i, (note_ms(i, bpm) / ms - 1.0).abs()))
        .filter(|(_, e)| *e < 0.02)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn ms_label(ms: f32) -> String {
    if ms < 100.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

/// The repeats picture: each repeat as a stem, left up and right down (so ping-pong and the
/// offset show), fading darker as the loop eats the top; plus the feedback meter.
fn repeats_window(
    p: &egui::Painter,
    r: Rect,
    engine: &Engine,
    v: Version,
    s: &VersionSettings,
    pal: &Palette,
    compact: bool,
) {
    p.rect_filled(r, CornerRadius::same(8), pal.card);
    let title = if compact { 11.5 } else { 13.0 };
    p.text(
        r.left_top() + vec2(10.0, 6.0),
        Align2::LEFT_TOP,
        "REPEATS",
        FontId::proportional(title),
        pal.accent,
    );
    let fb_w = (r.width() * 0.2).clamp(54.0, 84.0);
    let plot = Rect::from_min_max(
        pos2(r.left() + 10.0, r.top() + title + 14.0),
        pos2(r.right() - fb_w - 8.0, r.bottom() - 18.0),
    );
    let bpm = engine.meters().bpm();
    let d = s.delay_ms(bpm);
    let span = (d * 8.5).clamp(400.0, 12_000.0);
    let reps: Vec<Repeat> = curve::repeats(v, s, bpm, span);
    let mid = plot.center().y;
    let half = plot.height() / 2.0 - 4.0;
    p.line_segment(
        [pos2(plot.left(), mid), pos2(plot.right(), mid)],
        Stroke::new(1.0, pal.grid),
    );
    let x_of = |t: f32| plot.left() + plot.width() * (t / span).clamp(0.0, 1.0);
    let h_of = |db: f32| half * ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let font = FontId::proportional(10.0);
    p.text(
        pos2(plot.left(), plot.top()),
        Align2::LEFT_TOP,
        "L",
        font.clone(),
        pal.dim,
    );
    p.text(
        pos2(plot.left(), plot.bottom()),
        Align2::LEFT_BOTTOM,
        "R",
        font.clone(),
        pal.dim,
    );
    // The dry hit.
    let x = x_of(0.0) + 12.0;
    p.line_segment(
        [pos2(x, mid - half), pos2(x, mid + half)],
        Stroke::new(3.0, pal.before),
    );
    let offset_ms = s.offset_ms;
    let stem_w = (plot.width() / 90.0).clamp(2.0, 5.0);
    for rp in &reps {
        let colour = plate::lerp_colour(
            Color32::from_rgb(0x7A, 0x3C, 0x10),
            pal.after,
            rp.bright.clamp(0.0, 1.0),
        );
        let xl = 12.0 + x_of(rp.t_ms);
        let xr = 12.0 + x_of(rp.t_ms + offset_ms);
        let hl = h_of(rp.left_db);
        let hr = h_of(rp.right_db);
        if hl > 0.5 {
            p.line_segment(
                [pos2(xl, mid), pos2(xl, mid - hl)],
                Stroke::new(stem_w, colour),
            );
        }
        if hr > 0.5 {
            p.line_segment(
                [pos2(xr, mid), pos2(xr, mid + hr)],
                Stroke::new(stem_w, colour),
            );
        }
    }
    // Time axis.
    for k in 0..=4 {
        let t = span * k as f32 / 4.0;
        p.text(
            pos2(x_of(t) + if k == 0 { 12.0 } else { 0.0 }, r.bottom() - 3.0),
            if k == 4 {
                Align2::RIGHT_BOTTOM
            } else {
                Align2::CENTER_BOTTOM
            },
            if span >= 2000.0 {
                format!("{:.1} s", t / 1000.0)
            } else {
                format!("{t:.0} ms")
            },
            font.clone(),
            pal.dim,
        );
    }
    let mut tags = Vec::new();
    if s.reverse {
        tags.push("REVERSE");
    }
    if s.lofi {
        tags.push("LOFI");
    }
    if s.analog {
        tags.push("ANALOG");
    }
    if s.duck > 0.0 {
        tags.push("DUCK");
    }
    if !tags.is_empty() {
        p.text(
            pos2(plot.right(), r.top() + 6.0),
            Align2::RIGHT_TOP,
            tags.join(" · "),
            FontId::proportional(10.5),
            pal.sun,
        );
    }

    // Feedback meter: what's circulating in the loop, and how many repeats ring.
    let fr = Rect::from_min_max(
        pos2(r.right() - fb_w, r.top() + 6.0),
        r.max - vec2(6.0, 6.0),
    );
    p.text(
        fr.center_top(),
        Align2::CENTER_TOP,
        "FEEDBACK",
        FontId::proportional(10.0),
        pal.dim,
    );
    let level = engine.meters().loop_level();
    let ring = curve::ringing(v, s);
    let ring_txt = match ring {
        None => "SELF-OSC".to_owned(),
        Some(n) if n >= 99.0 => "99+ rpt".to_owned(),
        Some(n) => format!("{n:.0} rpt"),
    };
    let t = p.text(
        fr.center_bottom(),
        Align2::CENTER_BOTTOM,
        ring_txt,
        FontId::monospace(10.5),
        if ring.is_none() { pal.warn } else { pal.text },
    );
    let pct = p.text(
        pos2(fr.center().x, t.top() - 2.0),
        Align2::CENTER_BOTTOM,
        format!("{:.0}%", s.feedback * 100.0),
        FontId::proportional(10.5),
        pal.dim,
    );
    let bar_w = (fr.width() * 0.32).clamp(8.0, 20.0);
    let bar = Rect::from_min_max(
        pos2(fr.center().x - bar_w / 2.0, fr.top() + 16.0),
        pos2(fr.center().x + bar_w / 2.0, pct.top() - 4.0),
    );
    if bar.height() > 6.0 {
        p.rect_filled(bar, CornerRadius::same(3), pal.bg);
        let pos = ((level + 60.0) / 60.0).clamp(0.0, 1.0);
        let fill = Rect::from_min_max(pos2(bar.left(), bar.bottom() - bar.height() * pos), bar.max);
        p.rect_filled(
            fill,
            CornerRadius::same(3),
            if level > -3.0 { pal.warn } else { pal.after },
        );
    }
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

/// A waveform strip (BEFORE or AFTER): left and right in their own lanes, with an L/R
/// meter at the right end and the average level badge.
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
    let meter_w = (r.width() * 0.05).clamp(22.0, 40.0);
    let wave = Rect::from_min_max(r.min, pos2(r.right() - meter_w - 6.0, r.bottom()));
    let lane_h = wave.height() / 2.0;
    let small = r.height() < 50.0;
    let lane_font = FontId::proportional(if small { 9.5 } else { 10.5 });
    for ch in 0..2 {
        let lane = Rect::from_min_size(
            pos2(wave.left(), wave.top() + lane_h * ch as f32),
            vec2(wave.width(), lane_h),
        );
        let mid = lane.center().y;
        let half = lane.height() * 0.44;
        p.line_segment(
            [pos2(lane.left(), mid), pos2(lane.right(), mid)],
            Stroke::new(1.0, pal.grid),
        );
        let w = lane.width().max(1.0);
        let mut x = 0.0;
        while x < w {
            if let Some(c) = column_at(history, x, w) {
                let (lo, hi) = if after { c.after[ch] } else { c.before[ch] };
                if lo <= hi {
                    let y0 = mid - hi.clamp(-1.0, 1.0) * half;
                    let y1 = mid - lo.clamp(-1.0, 1.0) * half;
                    p.line_segment(
                        [
                            pos2(lane.left() + x, y0),
                            pos2(lane.left() + x, y1.max(y0 + 1.0)),
                        ],
                        Stroke::new(1.0, colour),
                    );
                }
            }
            x += 1.0;
        }
        p.text(
            pos2(lane.left() + 4.0, mid),
            Align2::LEFT_CENTER,
            ["L", "R"][ch],
            lane_font.clone(),
            pal.dim,
        );
    }
    let font = FontId::proportional(if small { 10.5 } else { 13.0 });
    // The title sits on its own chip so the waveform never runs through it.
    let g = p.layout_no_wrap(title.to_owned(), font, colour);
    let t = Rect::from_min_size(r.left_top() + vec2(8.0, 4.0), g.size());
    p.rect_filled(t.expand(2.0), CornerRadius::same(4), pal.card);
    p.galley(t.min, g, colour);
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

    // L/R peak meter.
    let (l, rr) = if after { m.output_lr() } else { m.input_lr() };
    let mr = Rect::from_min_max(
        pos2(r.right() - meter_w - 2.0, r.top() + 4.0),
        r.max - vec2(4.0, 4.0),
    );
    let bw = (mr.width() - 4.0) / 2.0;
    for (k, db) in [l, rr].into_iter().enumerate() {
        let bar = Rect::from_min_size(
            pos2(mr.left() + k as f32 * (bw + 4.0), mr.top() + 12.0),
            vec2(bw, (mr.height() - 12.0).max(4.0)),
        );
        p.text(
            pos2(bar.center().x, mr.top()),
            Align2::CENTER_TOP,
            ["L", "R"][k],
            FontId::proportional(9.5),
            pal.dim,
        );
        p.rect_filled(bar, CornerRadius::same(2), pal.bg);
        let pos = bar_pos(db);
        let fill = Rect::from_min_max(pos2(bar.left(), bar.bottom() - bar.height() * pos), bar.max);
        let c = if db > -3.0 {
            pal.warn
        } else if db > -12.0 {
            pal.sun
        } else {
            pal.good
        };
        p.rect_filled(fill, CornerRadius::same(2), c);
    }
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
