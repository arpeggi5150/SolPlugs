//! The hardware side: a playful parody panel per version, styled after a type of unit
//! (shapes, colours and layout of the type, never a copied logo or trademark).
//!
//! - **Tape**: a reel-to-reel tape echo: "TAPE ECHO" on top, two reels, cream knobs, the
//!   gold ☀ SolPlugs nameplate along the bottom.
//! - **Analog**: a dark bucket-brigade pedal box with a red LED and the chips on show.
//! - **Clean**: a flat modern digital face with faders, no vintage box.
//!
//! Each comes in two shapes: the slim strip along the bottom of the digital-first view,
//! and the taller panel the stacked view puts on top. Every control is a real parameter.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use solplugs_delay_core::{Ctl, Version};

use super::plate::{self, KnobLook, KnobOpts, Plate, knob};
use crate::engine::Engine;
use crate::params::{self, Pid};

/// Strip design canvas.
pub const STRIP_W: f32 = 1200.0;
/// Strip design height.
pub const STRIP_H: f32 = 150.0;
/// Panel design canvas.
pub const PANEL_W: f32 = 1000.0;
/// Panel design height.
pub const PANEL_H: f32 = 330.0;

/// A panel's colours and text.
struct Look {
    case: (Color32, Color32),
    panel: (Color32, Color32),
    brush: u8,
    metal: Color32,
    ink: Color32,
    accent: Color32,
    model: &'static str,
    knob: KnobLook,
}

/// The gold of the Tape nameplate.
const GOLD: (Color32, Color32) = (
    Color32::from_rgb(0xE8, 0xC9, 0x7A),
    Color32::from_rgb(0xA8, 0x82, 0x3A),
);

fn look(v: Version) -> Look {
    let knob = |body, cap, shine, pointer, ink, ridges| KnobLook {
        body,
        cap,
        shine,
        pointer,
        ink,
        ridges,
        ticks: true,
    };
    match v {
        Version::Tape => Look {
            case: (
                Color32::from_rgb(0x26, 0x28, 0x2C),
                Color32::from_rgb(0x15, 0x16, 0x19),
            ),
            panel: (
                Color32::from_rgb(0x2C, 0x2F, 0x34),
                Color32::from_rgb(0x1B, 0x1D, 0x21),
            ),
            brush: 10,
            metal: Color32::from_rgb(0xA8, 0xAA, 0xAE),
            ink: Color32::from_rgb(0xE4, 0xE6, 0xEA),
            accent: Color32::from_rgb(0x2B, 0xC9, 0xBA),
            model: "TAPE ECHO",
            knob: knob(
                Color32::from_rgb(0x16, 0x17, 0x1A),
                Color32::from_rgb(0x26, 0x28, 0x2D),
                Color32::from_white_alpha(26),
                Color32::from_rgb(0xF2, 0xF2, 0xF2),
                Color32::from_rgb(0xD6, 0xD9, 0xDE),
                0,
            ),
        },
        Version::Analog => Look {
            case: (
                Color32::from_rgb(0x2A, 0x14, 0x18),
                Color32::from_rgb(0x14, 0x08, 0x0A),
            ),
            panel: (
                Color32::from_rgb(0x3A, 0x1C, 0x22),
                Color32::from_rgb(0x22, 0x0E, 0x12),
            ),
            brush: 3,
            metal: Color32::from_rgb(0x9C, 0x96, 0x92),
            ink: Color32::from_rgb(0xF1, 0xE2, 0xD2),
            accent: Color32::from_rgb(0xFF, 0x3B, 0x30),
            model: "ANALOG DELAY · BUCKET BRIGADE",
            knob: knob(
                Color32::from_rgb(0x12, 0x10, 0x10),
                Color32::from_rgb(0x24, 0x20, 0x20),
                Color32::from_white_alpha(22),
                Color32::from_rgb(0xF1, 0xE2, 0xD2),
                Color32::from_rgb(0xF1, 0xE2, 0xD2),
                20,
            ),
        },
        Version::Clean => Look {
            case: (
                Color32::from_rgb(0x1C, 0x1F, 0x25),
                Color32::from_rgb(0x1C, 0x1F, 0x25),
            ),
            panel: (
                Color32::from_rgb(0x22, 0x26, 0x2D),
                Color32::from_rgb(0x22, 0x26, 0x2D),
            ),
            brush: 0,
            metal: Color32::from_rgb(0x55, 0x5C, 0x66),
            ink: Color32::from_rgb(0xDD, 0xE3, 0xEA),
            accent: Color32::from_rgb(0x1F, 0xE0, 0xD0),
            model: "CLEAN · DIGITAL DELAY",
            knob: knob(
                Color32::BLACK,
                Color32::BLACK,
                Color32::BLACK,
                Color32::WHITE,
                Color32::WHITE,
                0,
            ),
        },
    }
}

/// One control on the panel.
#[derive(Clone, Copy)]
enum Item {
    C(Ctl, &'static str),
    X(usize),
}

fn items(v: Version) -> Vec<Item> {
    let mut list = vec![
        Item::C(Ctl::Feedback, "FEEDBACK"),
        Item::C(Ctl::FilterHp, "FILTER HP"),
        Item::C(Ctl::FilterLp, "FILTER LP"),
        Item::C(Ctl::Wet, "WET"),
        Item::C(Ctl::Dry, "DRY"),
        Item::C(Ctl::LowCut, "LOW CUT"),
        Item::C(Ctl::HighCut, "HIGH CUT"),
    ];
    match v {
        Version::Tape => list.push(Item::C(Ctl::Mod, "WOW/FLUTTER")),
        Version::Analog => list.push(Item::C(Ctl::Mod, "CHORUS")),
        Version::Clean => {}
    }
    for i in 0..v.extras().len() {
        list.push(Item::X(i));
    }
    list
}

fn item_id(v: Version, it: Item) -> Pid {
    match it {
        Item::C(c, _) => Pid::c(v, c),
        Item::X(i) => Pid::x(v, i),
    }
}

fn item_label(v: Version, it: Item) -> String {
    match it {
        Item::C(_, l) => l.to_owned(),
        Item::X(i) => v
            .extras()
            .get(i)
            .map_or(String::new(), |s| s.name.to_uppercase()),
    }
}

/// Draws the active version's hardware into `area`: the panel when `tall`, else the strip.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, tall: bool) {
    let v = engine.version();
    let (w, h) = if tall {
        (PANEL_W, PANEL_H)
    } else {
        (STRIP_W, STRIP_H)
    };
    let mut pl = Plate::new(ui, area, w, h);
    let lk = look(v);
    frame(&pl, v, &lk);
    let list = items(v);
    // Controls run from the nameplate to the button block on the right.
    let (x0, x1) = if tall {
        (30.0, w - 150.0)
    } else {
        (250.0, w - 140.0)
    };
    let y = if tall { 196.0 } else { 58.0 };
    let r = if tall { 27.0 } else { 22.0 };
    let step = (x1 - x0) / list.len() as f32;
    for (k, it) in list.iter().enumerate() {
        let x = x0 + step * (k as f32 + 0.5);
        let id = item_id(v, *it);
        let label = item_label(v, *it);
        let help = id.info().map_or("", |i| i.help);
        if v == Version::Clean {
            fader(&mut pl, engine, id, (x, y, r), &label, &lk);
        } else {
            knob(
                &mut pl,
                engine,
                id,
                (x, y, r.min(step * 0.36)),
                &lk.knob,
                KnobOpts {
                    label: &label,
                    help,
                    ..Default::default()
                },
            );
        }
    }
    // Right block: LOFI, ANALOG and REVERSE.
    let (bx, by, bw, bh) = if tall {
        (w - 135.0, 140.0, 112.0, 34.0)
    } else {
        (w - 128.0, 16.0, 112.0, 34.0)
    };
    for (k, (c, label)) in [
        (Ctl::LoFi, "LOFI"),
        (Ctl::Analog, "ANALOG"),
        (Ctl::Reverse, "REVERSE"),
    ]
    .into_iter()
    .enumerate()
    {
        toggle(
            &mut pl,
            engine,
            Pid::c(v, c),
            (bx, by + k as f32 * (bh + 6.0), bw, bh),
            label,
            &lk,
        );
    }
}

/// Case, panel, screws, the nameplate and the version's decoration.
fn frame(pl: &Plate<'_>, v: Version, lk: &Look) {
    let (w, h) = (pl.w, pl.h);
    let tall = h > 200.0;
    let radius = if v == Version::Clean { 10.0 } else { 8.0 };
    pl.slab(pl.rect, lk.case.0, lk.case.1, lk.brush, radius);
    pl.slab(
        pl.r(8.0, 8.0, w - 16.0, h - 16.0),
        lk.panel.0,
        lk.panel.1,
        lk.brush,
        6.0,
    );
    if v != Version::Clean {
        pl.corner_screws(8.0, 8.0, w - 16.0, h - 16.0, 14.0, lk.metal);
    }
    match (v, tall) {
        (Version::Tape, true) => {
            // As in the draft: TAPE ECHO across the top, the reels, and the gold nameplate
            // (the one place SolPlugs appears on the face) along the bottom.
            pl.text_at(
                w / 2.0,
                30.0,
                Align2::CENTER_CENTER,
                "T A P E   E C H O",
                pl.font(24.0),
                lk.ink,
            );
            reel(pl, 150.0, 92.0, 46.0, lk);
            reel(pl, 850.0, 92.0, 46.0, lk);
            tape_path(pl, lk);
            gold_nameplate(pl, pl.r(330.0, 282.0, 340.0, 36.0));
        }
        (Version::Tape, false) => {
            pl.text_at(
                130.0,
                34.0,
                Align2::CENTER_CENTER,
                "T A P E   E C H O",
                pl.font(15.0),
                lk.ink,
            );
            reel(pl, 60.0, 92.0, 30.0, lk);
            reel(pl, 200.0, 92.0, 30.0, lk);
            gold_nameplate(pl, pl.r(84.0, 120.0, 92.0, 22.0));
        }
        _ => {
            // Nameplate: ☀ SolPlugs and the type line.
            let (nx, ny) = if tall { (40.0, 40.0) } else { (30.0, 40.0) };
            sun(
                pl,
                nx + 12.0,
                ny,
                13.0,
                if v == Version::Clean {
                    lk.accent
                } else {
                    lk.ink
                },
            );
            pl.text_at(
                nx + 32.0,
                ny,
                Align2::LEFT_CENTER,
                "SolPlugs",
                pl.font(if tall { 32.0 } else { 26.0 }),
                lk.ink,
            );
            let plaque = if tall {
                pl.r(nx, ny + 28.0, 330.0, 26.0)
            } else {
                pl.r(nx - 6.0, ny + 34.0, 214.0, 22.0)
            };
            pl.plaque(
                plaque,
                lk.model,
                if tall { 12.5 } else { 9.5 },
                plate::lerp_colour(lk.panel.1, Color32::BLACK, 0.4),
                lk.metal,
                lk.accent,
            );
            if tall {
                decoration(pl, v, lk);
            }
        }
    }
}

/// The gold SolPlugs nameplate with reel emblems at each end.
fn gold_nameplate(pl: &Plate<'_>, r: Rect) {
    let p = &pl.painter;
    let cr = CornerRadius::same(pl.s(6.0) as u8);
    p.rect_filled(
        r.translate(vec2(0.0, pl.s(2.0))),
        cr,
        Color32::from_black_alpha(150),
    );
    let bands = 10;
    for i in 0..bands {
        let t = i as f32 / bands as f32;
        let c = plate::lerp_colour(GOLD.0, GOLD.1, (t * 2.0 - 1.0).abs());
        let y0 = r.top() + r.height() * t;
        let y1 = r.top() + r.height() * (t + 1.0 / bands as f32) + 0.5;
        p.rect_filled(
            Rect::from_x_y_ranges(r.x_range(), y0..=y1),
            CornerRadius::ZERO,
            c,
        );
    }
    p.rect_stroke(
        r,
        cr,
        Stroke::new(pl.s(1.5).max(1.0), Color32::from_rgb(0x6E, 0x52, 0x1E)),
        StrokeKind::Inside,
    );
    let ink = Color32::from_rgb(0x4A, 0x34, 0x10);
    let rr = r.height() * 0.32;
    for x in [r.left() + r.height() * 0.5, r.right() - r.height() * 0.5] {
        let c = pos2(x, r.center().y);
        p.circle_stroke(c, rr, Stroke::new(1.2, ink));
        for k in 0..3 {
            let a = k as f32 * std::f32::consts::TAU / 3.0;
            p.circle_filled(c + vec2(a.cos(), a.sin()) * rr * 0.5, rr * 0.22, ink);
        }
    }
    let size = (r.height() * 0.58).max(plate::MIN_TEXT);
    let g = p.layout_no_wrap("SolPlugs".into(), FontId::proportional(size), ink);
    let total = g.size().x + size * 1.2;
    let left = r.center().x - total / 2.0;
    let sc = pos2(left + size * 0.45, r.center().y);
    p.circle_filled(sc, size * 0.18, ink);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = vec2(a.cos(), a.sin());
        p.line_segment(
            [sc + d * size * 0.28, sc + d * size * 0.42],
            Stroke::new(1.0, ink),
        );
    }
    p.galley(
        pos2(left + size * 1.2, r.center().y - g.size().y / 2.0),
        g,
        ink,
    );
}

/// A tape reel: flange, three-spoke hub, tape wound on.
fn reel(pl: &Plate<'_>, x: f32, y: f32, r: f32, lk: &Look) {
    let p = &pl.painter;
    let c = pl.p(x, y);
    let rr = pl.s(r);
    p.circle_filled(
        c + vec2(0.0, rr * 0.06),
        rr * 1.02,
        Color32::from_black_alpha(150),
    );
    p.circle_filled(c, rr, Color32::from_rgb(0x9A, 0x9C, 0xA2));
    p.circle_filled(c, rr * 0.86, Color32::from_rgb(0x3A, 0x2A, 0x1E));
    p.circle_filled(c, rr * 0.42, Color32::from_rgb(0xB4, 0xB6, 0xBC));
    for k in 0..3 {
        let a = k as f32 * std::f32::consts::TAU / 3.0 + 0.4;
        let d = vec2(a.cos(), a.sin());
        p.circle_filled(c + d * rr * 0.62, rr * 0.16, lk.panel.1);
    }
    p.circle_filled(c, rr * 0.12, lk.panel.1);
    p.circle_stroke(c, rr, Stroke::new(1.0, Color32::from_black_alpha(160)));
}

/// The tape running from reel to reel past the heads.
fn tape_path(pl: &Plate<'_>, lk: &Look) {
    let p = &pl.painter;
    let tape = Color32::from_rgb(0x5A, 0x40, 0x2C);
    let pts = [
        pl.p(150.0, 138.0),
        pl.p(380.0, 128.0),
        pl.p(620.0, 128.0),
        pl.p(850.0, 138.0),
    ];
    p.add(egui::Shape::line(
        pts.to_vec(),
        Stroke::new(pl.s(3.0).max(1.5), tape),
    ));
    for (k, x) in [430.0, 500.0, 570.0].into_iter().enumerate() {
        let r = pl.r(x - 16.0, 112.0, 32.0, 22.0);
        p.rect_filled(r, CornerRadius::same(3), lk.metal);
        pl.text(
            x,
            146.0,
            ["REC", "PB 1", "PB 2"][k],
            9.5,
            lk.ink.gamma_multiply(0.7),
        );
    }
}

/// The ☀ mark.
fn sun(pl: &Plate<'_>, x: f32, y: f32, r: f32, c: Color32) {
    let p = pl.p(x, y);
    let rr = pl.s(r);
    pl.painter.circle_filled(p, rr * 0.45, c);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = vec2(a.cos(), a.sin());
        pl.painter.line_segment(
            [p + d * rr * 0.65, p + d * rr],
            Stroke::new(pl.s(2.2).max(1.0), c),
        );
    }
}

/// The tall panel's centre piece for Analog and Clean.
fn decoration(pl: &Plate<'_>, v: Version, lk: &Look) {
    let p = &pl.painter;
    match v {
        Version::Analog => {
            // A status LED and the bucket-brigade chips on a little board.
            let led = pl.p(430.0, 54.0);
            p.circle_filled(led, pl.s(12.0), lk.accent.gamma_multiply(0.25));
            p.circle_filled(led, pl.s(6.0), lk.accent);
            pl.text(430.0, 80.0, "ON", 10.0, lk.ink);
            let board = pl.r(480.0, 26.0, 470.0, 100.0);
            p.rect_filled(
                board,
                CornerRadius::same(4),
                Color32::from_rgb(0x1E, 0x3A, 0x26),
            );
            for (k, label) in ["BBD", "BBD", "CLOCK", "COMP"].into_iter().enumerate() {
                let x = 520.0 + k as f32 * 112.0;
                let chip = pl.r(x, 52.0, 78.0, 36.0);
                p.rect_filled(chip, CornerRadius::same(2), Color32::from_gray(18));
                for pin in 0..6 {
                    let px = x + 8.0 + pin as f32 * 12.0;
                    for py in [48.0, 88.0] {
                        p.rect_filled(pl.r(px, py, 4.0, 4.0), CornerRadius::ZERO, lk.metal);
                    }
                }
                pl.text(x + 39.0, 70.0, label, 10.0, lk.ink.gamma_multiply(0.8));
            }
            pl.text(
                715.0,
                112.0,
                "4096 STAGES",
                10.0,
                lk.ink.gamma_multiply(0.6),
            );
        }
        Version::Clean => {
            let r = pl.r(420.0, 30.0, 540.0, 90.0);
            p.rect_stroke(
                r,
                CornerRadius::same(8),
                Stroke::new(1.0, lk.metal),
                StrokeKind::Inside,
            );
            pl.text(
                690.0,
                75.0,
                "Clean digital delay · exact repeats · no colour",
                13.0,
                lk.ink.gamma_multiply(0.7),
            );
        }
        Version::Tape => {}
    }
}

/// A lamp button for an on/off parameter.
fn toggle(
    pl: &mut Plate<'_>,
    engine: &Engine,
    id: Pid,
    (x, y, w, h): (f32, f32, f32, f32),
    label: &str,
    lk: &Look,
) {
    let on = engine.params().on(id);
    let rect = pl.r(x, y, w, h);
    let face = plate::lerp_colour(lk.panel.1, Color32::BLACK, 0.3);
    let resp = plate::button(pl, (label, id.0), rect, label, on, face, lk.accent, lk.ink);
    if resp
        .on_hover_text(id.info().map_or("", |i| i.help))
        .clicked()
    {
        engine.set(id, if on { 0.0 } else { 1.0 });
    }
}

/// Clean's flat fader: a vertical track with a thumb, the label and value under it.
fn fader(
    pl: &mut Plate<'_>,
    engine: &Engine,
    id: Pid,
    (x, y, r): (f32, f32, f32),
    label: &str,
    lk: &Look,
) {
    let Some(info) = id.info() else {
        return;
    };
    let track = pl.r(x - 3.0, y - r * 1.4, 6.0, r * 2.8);
    let hit = track.expand2(vec2(pl.s(18.0), pl.s(6.0)));
    let resp = pl
        .ui
        .interact(
            hit,
            pl.ui.scope_id().with(("fader", id.0)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeVertical);
    let value = engine.params().get(id);
    let norm = info.normalize(value);
    plate::drag_logic(pl.ui, engine, id, &resp, norm, false);
    let p = &pl.painter;
    p.rect_filled(
        track,
        CornerRadius::same(3),
        Color32::from_rgb(0x12, 0x14, 0x18),
    );
    let fill_top = track.bottom() - track.height() * norm as f32;
    p.rect_filled(
        Rect::from_min_max(pos2(track.left(), fill_top), track.max),
        CornerRadius::same(3),
        lk.accent.gamma_multiply(0.75),
    );
    let thumb = Rect::from_center_size(
        pos2(track.center().x, fill_top),
        vec2(pl.s(26.0), pl.s(9.0)),
    );
    p.rect_filled(
        thumb,
        CornerRadius::same(3),
        if resp.hovered() || resp.dragged() {
            Color32::WHITE
        } else {
            lk.ink
        },
    );
    let ty = y + r * 1.4 + 12.0;
    pl.bold(x, ty, label, 12.0, lk.ink);
    pl.text(
        x,
        ty + 15.0,
        &params::format(id, value),
        11.0,
        lk.ink.gamma_multiply(0.8),
    );
    resp.on_hover_text(format!(
        "{}: {}\n{}",
        info.name,
        params::format(id, value),
        info.help
    ));
}
