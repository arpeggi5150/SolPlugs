//! The hardware side: a playful parody panel per version, styled after a type of unit
//! (shapes, colours and layout of the type, never a copied logo or trademark).
//!
//! - **Hall**: a dark 80s rack unit with a red LED display.
//! - **Room**: a studio room box with wooden cheeks.
//! - **Plate**: a big steel plate in its frame, with the two pickups.
//! - **Spring**: a tube spring tank: tolex, a glowing tube, the springs on show.
//! - **Clean**: a flat modern digital face with faders, no vintage box.
//!
//! Each comes in two shapes: the slim strip along the bottom of the digital-first view,
//! and the taller panel the stacked view puts on top. Every control is a real parameter.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use solplugs_reverb_core::{Ctl, Version};

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
        Version::Hall => Look {
            case: (
                Color32::from_rgb(0x1B, 0x1B, 0x1E),
                Color32::from_rgb(0x0D, 0x0D, 0x0F),
            ),
            panel: (
                Color32::from_rgb(0x24, 0x24, 0x28),
                Color32::from_rgb(0x15, 0x15, 0x18),
            ),
            brush: 14,
            metal: Color32::from_rgb(0x8A, 0x8A, 0x90),
            ink: Color32::from_rgb(0xE9, 0xD9, 0xB0),
            accent: Color32::from_rgb(0xFF, 0x3B, 0x30),
            model: "RACK REVERB · 1986 · DIGITAL HALL",
            knob: knob(
                Color32::from_rgb(0x1A, 0x1A, 0x1C),
                Color32::from_rgb(0x2C, 0x2C, 0x30),
                Color32::from_white_alpha(28),
                Color32::from_rgb(0xE9, 0xD9, 0xB0),
                Color32::from_rgb(0xE9, 0xD9, 0xB0),
                24,
            ),
        },
        Version::Room => Look {
            case: (
                Color32::from_rgb(0x6B, 0x41, 0x22),
                Color32::from_rgb(0x45, 0x29, 0x14),
            ),
            panel: (
                Color32::from_rgb(0xD8, 0xCF, 0xBA),
                Color32::from_rgb(0xBF, 0xB4, 0x9C),
            ),
            brush: 6,
            metal: Color32::from_rgb(0x9A, 0x96, 0x8C),
            ink: Color32::from_rgb(0x2A, 0x24, 0x1C),
            accent: Color32::from_rgb(0xD0, 0x5A, 0x1E),
            model: "STUDIO ROOM · ACOUSTIC SIMULATOR",
            knob: knob(
                Color32::from_rgb(0x30, 0x2C, 0x28),
                Color32::from_rgb(0x48, 0x42, 0x3C),
                Color32::from_white_alpha(30),
                Color32::from_rgb(0xF0, 0xE8, 0xD8),
                Color32::from_rgb(0x2A, 0x24, 0x1C),
                0,
            ),
        },
        Version::Plate => Look {
            case: (
                Color32::from_rgb(0x55, 0x5C, 0x63),
                Color32::from_rgb(0x34, 0x39, 0x3F),
            ),
            panel: (
                Color32::from_rgb(0xB9, 0xC0, 0xC6),
                Color32::from_rgb(0x8E, 0x96, 0x9E),
            ),
            brush: 22,
            metal: Color32::from_rgb(0xC8, 0xCC, 0xD0),
            ink: Color32::from_rgb(0x1E, 0x24, 0x2A),
            accent: Color32::from_rgb(0xE8, 0xC4, 0x5A),
            model: "STEEL PLATE · 2 PICKUPS · ECHO PLATE",
            knob: knob(
                Color32::from_rgb(0x2C, 0x30, 0x34),
                Color32::from_rgb(0x46, 0x4C, 0x52),
                Color32::from_white_alpha(40),
                Color32::from_rgb(0xF4, 0xF4, 0xF0),
                Color32::from_rgb(0x1E, 0x24, 0x2A),
                18,
            ),
        },
        Version::Spring => Look {
            case: (
                Color32::from_rgb(0x20, 0x3A, 0x38),
                Color32::from_rgb(0x11, 0x22, 0x21),
            ),
            panel: (
                Color32::from_rgb(0x2B, 0x2E, 0x2C),
                Color32::from_rgb(0x1C, 0x1E, 0x1D),
            ),
            brush: 4,
            metal: Color32::from_rgb(0xB8, 0xB0, 0x98),
            ink: Color32::from_rgb(0xF2, 0xE6, 0xC4),
            accent: Color32::from_rgb(0xFF, 0x9A, 0x3C),
            model: "TUBE SPRING TANK · 3 SPRINGS",
            knob: knob(
                Color32::from_rgb(0xE9, 0xE2, 0xCC),
                Color32::from_rgb(0xF6, 0xF0, 0xDE),
                Color32::from_white_alpha(70),
                Color32::from_rgb(0x20, 0x1C, 0x18),
                Color32::from_rgb(0xF2, 0xE6, 0xC4),
                0,
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
            model: "CLEAN · DIGITAL",
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
        Item::C(Ctl::Decay, "DECAY"),
        Item::C(
            Ctl::Size,
            if v == Version::Spring {
                "LENGTH"
            } else {
                "SIZE"
            },
        ),
        Item::C(Ctl::PreDelay, "PRE-DELAY"),
        Item::C(Ctl::Damping, "HI DAMP"),
        Item::C(Ctl::LowDamping, "LO DAMP"),
        Item::C(
            Ctl::Diffusion,
            if v == Version::Spring {
                "DRIP"
            } else {
                "DIFFUSION"
            },
        ),
    ];
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
    // Controls run from the nameplate to the display/freeze block on the right.
    let (x0, x1) = if tall {
        (40.0, w - 190.0)
    } else {
        (260.0, w - 150.0)
    };
    let y = if tall { 238.0 } else { 58.0 };
    let r = if tall { 30.0 } else { 23.0 };
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
                (x, y, r),
                &lk.knob,
                KnobOpts {
                    label: &label,
                    help,
                    ..Default::default()
                },
            );
        }
    }
    // Right block: decay display and freeze.
    let (bx, by, bw) = if tall {
        (w - 170.0, 180.0, 140.0)
    } else {
        (w - 135.0, 20.0, 118.0)
    };
    display(&pl, engine, v, (bx, by, bw, 40.0), &lk);
    freeze_button(
        &mut pl,
        engine,
        v,
        (bx, by + 54.0, bw, if tall { 46.0 } else { 40.0 }),
        &lk,
    );
}

/// Case, panel, screws, the nameplate and the version's decoration.
fn frame(pl: &Plate<'_>, v: Version, lk: &Look) {
    let (w, h) = (pl.w, pl.h);
    let tall = h > 200.0;
    let radius = if v == Version::Clean { 10.0 } else { 8.0 };
    pl.slab(pl.rect, lk.case.0, lk.case.1, lk.brush, radius);
    let inset = if v == Version::Room { 26.0 } else { 8.0 };
    pl.slab(
        pl.r(inset, 8.0, w - 2.0 * inset, h - 16.0),
        lk.panel.0,
        lk.panel.1,
        lk.brush,
        6.0,
    );
    if v == Version::Hall {
        // Rack ears with mounting slots.
        for x in [14.0, w - 14.0] {
            for yy in [h * 0.25, h * 0.75] {
                pl.painter.rect_filled(
                    Rect::from_center_size(pl.p(x, yy), vec2(pl.s(6.0), pl.s(16.0))),
                    CornerRadius::same(3),
                    Color32::from_black_alpha(200),
                );
            }
        }
    } else if v != Version::Clean {
        pl.corner_screws(inset, 8.0, w - 2.0 * inset, h - 16.0, 14.0, lk.metal);
    }
    // Nameplate: ☀ SolPlugs and the type line.
    let (nx, ny) = if tall { (48.0, 44.0) } else { (36.0, 40.0) };
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
        pl.font(if tall { 34.0 } else { 28.0 }),
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
        if tall { 12.5 } else { 10.0 },
        plate::lerp_colour(lk.panel.1, Color32::BLACK, 0.4),
        lk.metal,
        lk.accent,
    );
    if tall {
        decoration(pl, v, lk);
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

/// The tall panel's centre piece for each version.
fn decoration(pl: &Plate<'_>, v: Version, lk: &Look) {
    let w = pl.w;
    let p = &pl.painter;
    match v {
        Version::Plate => {
            // The plate: a big sheet in its frame, hung on springs, with two pickups.
            let sheet = pl.r(420.0, 30.0, 520.0, 130.0);
            p.rect_filled(
                sheet.expand(pl.s(6.0)),
                CornerRadius::same(4),
                Color32::from_rgb(0x2E, 0x33, 0x38),
            );
            p.rect_filled(
                sheet,
                CornerRadius::same(2),
                Color32::from_rgb(0xD4, 0xD8, 0xDC),
            );
            for k in 0..14 {
                let y = sheet.top() + sheet.height() * (k as f32 + 0.5) / 14.0;
                p.line_segment(
                    [pos2(sheet.left(), y), pos2(sheet.right(), y)],
                    Stroke::new(1.0, Color32::from_black_alpha(18)),
                );
            }
            for (sx, sy) in [(420.0, 30.0), (940.0, 30.0), (420.0, 160.0), (940.0, 160.0)] {
                coil(
                    pl,
                    (sx, sy),
                    (
                        sx + if sx < 600.0 { -14.0 } else { 14.0 },
                        sy + if sy < 100.0 { -14.0 } else { 14.0 },
                    ),
                    lk.metal,
                    5,
                );
            }
            for (cx, label) in [(540.0, "PICKUP L"), (820.0, "PICKUP R")] {
                let c = pl.p(cx, 95.0);
                p.circle_filled(c, pl.s(20.0), Color32::from_rgb(0x30, 0x30, 0x32));
                p.circle_filled(c, pl.s(12.0), lk.accent);
                p.circle_stroke(
                    c,
                    pl.s(20.0),
                    Stroke::new(pl.s(2.0), Color32::from_black_alpha(160)),
                );
                pl.text(cx, 130.0, label, 10.0, lk.ink);
            }
            pl.text(680.0, 95.0, "DRIVER", 10.0, lk.ink.gamma_multiply(0.7));
        }
        Version::Spring => {
            // Tube and the springs.
            let tube = pl.r(430.0, 30.0, 46.0, 110.0);
            p.rect_filled(
                tube,
                CornerRadius::same(20),
                Color32::from_rgba_unmultiplied(220, 220, 220, 40),
            );
            p.circle_filled(
                pl.p(453.0, 100.0),
                pl.s(22.0),
                Color32::from_rgba_unmultiplied(255, 140, 40, 60),
            );
            p.circle_filled(
                pl.p(453.0, 100.0),
                pl.s(9.0),
                Color32::from_rgb(0xFF, 0x9A, 0x3C),
            );
            p.rect_stroke(
                tube,
                CornerRadius::same(20),
                Stroke::new(pl.s(1.5), Color32::from_white_alpha(80)),
                StrokeKind::Inside,
            );
            pl.text(453.0, 152.0, "TUBE", 10.0, lk.ink);
            let tank = pl.r(510.0, 36.0, w - 560.0, 110.0);
            p.rect_filled(
                tank,
                CornerRadius::same(4),
                Color32::from_rgb(0x15, 0x17, 0x16),
            );
            for (k, y) in [62.0, 92.0, 122.0].into_iter().enumerate() {
                coil(pl, (520.0, y), (w - 60.0, y), lk.metal, 34 + k * 3);
            }
        }
        Version::Hall => {
            // A row of LEDs and a program wheel, rack style.
            for k in 0..8 {
                let c = pl.p(470.0 + k as f32 * 22.0, 96.0);
                let lit = k < 5;
                p.circle_filled(
                    c,
                    pl.s(5.0),
                    if lit {
                        lk.accent
                    } else {
                        Color32::from_rgb(0x40, 0x10, 0x10)
                    },
                );
            }
            pl.text(547.0, 120.0, "PROGRAM · HALL A", 10.0, lk.ink);
            p.rect_filled(
                pl.r(680.0, 70.0, 260.0, 52.0),
                CornerRadius::same(4),
                Color32::from_rgb(0x0A, 0x0A, 0x0A),
            );
            pl.text(
                810.0,
                96.0,
                "· · ·  LARGE HALL  · · ·",
                12.0,
                lk.accent.gamma_multiply(0.8),
            );
        }
        Version::Room => {
            // A little floor plan of the room with a source and a listener.
            let room = pl.r(470.0, 34.0, 440.0, 120.0);
            p.rect_filled(
                room,
                CornerRadius::same(3),
                Color32::from_rgb(0xEC, 0xE6, 0xD6),
            );
            p.rect_stroke(
                room,
                CornerRadius::same(3),
                Stroke::new(pl.s(3.0), lk.ink),
                StrokeKind::Inside,
            );
            let src = pl.p(540.0, 94.0);
            let ear = pl.p(840.0, 94.0);
            p.circle_filled(src, pl.s(7.0), lk.accent);
            p.circle_filled(ear, pl.s(7.0), lk.ink);
            for a in [
                pl.p(690.0, 34.0),
                pl.p(690.0, 154.0),
                pl.p(470.0, 94.0),
                pl.p(910.0, 60.0),
            ] {
                p.line_segment([src, a], Stroke::new(1.0, lk.accent.gamma_multiply(0.6)));
                p.line_segment([a, ear], Stroke::new(1.0, lk.accent.gamma_multiply(0.6)));
            }
            p.line_segment([src, ear], Stroke::new(1.2, lk.ink.gamma_multiply(0.5)));
            pl.text(690.0, 168.0, "EARLY REFLECTIONS", 10.0, lk.ink);
        }
        Version::Clean => {
            let r = pl.r(440.0, 40.0, 500.0, 110.0);
            p.rect_stroke(
                r,
                CornerRadius::same(8),
                Stroke::new(1.0, lk.metal),
                StrokeKind::Inside,
            );
            pl.text(
                690.0,
                95.0,
                "Clean digital reverb · 16-line network",
                13.0,
                lk.ink.gamma_multiply(0.7),
            );
        }
    }
}

/// A drawn coil spring from `a` to `b` with `turns` loops.
fn coil(pl: &Plate<'_>, a: (f32, f32), b: (f32, f32), c: Color32, turns: usize) {
    let (ax, ay) = a;
    let (bx, by) = b;
    let len = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt().max(1.0);
    let (ux, uy) = ((bx - ax) / len, (by - ay) / len);
    let (nx, ny) = (-uy, ux);
    let amp = 5.0;
    let n = turns * 8;
    let pts: Vec<_> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let s = (t * turns as f32 * std::f32::consts::TAU).sin() * amp;
            pl.p(ax + ux * len * t + nx * s, ay + uy * len * t + ny * s)
        })
        .collect();
    pl.painter
        .add(egui::Shape::line(pts, Stroke::new(pl.s(1.6).max(1.0), c)));
}

/// The small display: the reverb time, like the unit's readout.
fn display(
    pl: &Plate<'_>,
    engine: &Engine,
    v: Version,
    (x, y, w, h): (f32, f32, f32, f32),
    lk: &Look,
) {
    let r = pl.r(x, y, w, h);
    let p = &pl.painter;
    let (bg, ink) = match v {
        Version::Hall => (Color32::from_rgb(0x12, 0x02, 0x02), lk.accent),
        Version::Clean => (Color32::from_rgb(0x12, 0x16, 0x1B), lk.accent),
        _ => (
            Color32::from_rgb(0x0E, 0x10, 0x0E),
            Color32::from_rgb(0x9C, 0xF0, 0x7A),
        ),
    };
    p.rect_filled(r, CornerRadius::same(pl.s(4.0) as u8), bg);
    p.rect_stroke(
        r,
        CornerRadius::same(pl.s(4.0) as u8),
        Stroke::new(pl.s(1.5).max(1.0), Color32::from_black_alpha(200)),
        StrokeKind::Inside,
    );
    let id = Pid::c(v, Ctl::Decay);
    let frozen = engine.params().on(Pid::c(v, Ctl::Freeze));
    let text = if frozen {
        "FREEZE".to_owned()
    } else {
        params::format(id, engine.params().get(id))
    };
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace((pl.s(h * 0.5)).max(10.0)),
        ink,
    );
}

/// The Freeze button: lit while the tail is held.
fn freeze_button(
    pl: &mut Plate<'_>,
    engine: &Engine,
    v: Version,
    (x, y, w, h): (f32, f32, f32, f32),
    lk: &Look,
) {
    let id = Pid::c(v, Ctl::Freeze);
    let on = engine.params().on(id);
    let rect = pl.r(x, y, w, h);
    let face = plate::lerp_colour(lk.panel.1, Color32::BLACK, 0.3);
    let resp = plate::button(
        pl,
        ("freeze", v as u32),
        rect,
        "FREEZE",
        on,
        face,
        lk.accent,
        lk.ink,
    );
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
