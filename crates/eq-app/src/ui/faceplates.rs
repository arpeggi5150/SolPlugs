//! The seven hardware faceplates. Each is a parody panel styled after a type of unit:
//! the shapes, colours and layout of the type, never a copied logo or trademark. Every
//! control is a real parameter: knobs for continuous values and stepped choices, lamp
//! buttons for switches. Hover any control for its help line.

use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui};
use solplugs_eq_core::style::{Unit, api, curve, maag, massive, neve, pultec, ssl};
use solplugs_eq_core::Style;

use super::plate::{self, H, KnobLook, KnobOpts, Plate, W, knob, vu_meter, vu_pos};
use crate::engine::Engine;
use crate::params::{C, Pid};

/// One control on a panel, at design point (x, y).
#[derive(Clone, Copy)]
enum Item {
    /// Knob `i` with a label.
    K(usize, &'static str, f32, f32),
    /// Switch `i` with a caption.
    S(usize, &'static str, f32, f32),
}

/// A panel's colours and text.
#[derive(Clone, Copy)]
struct Look {
    /// Outer case, top and bottom.
    case: (Color32, Color32),
    /// Inner panel, top and bottom.
    panel: (Color32, Color32),
    /// Brushed-metal amount.
    brush: u8,
    /// Screws.
    metal: Color32,
    /// Lettering.
    ink: Color32,
    /// Lit switch colour.
    lit: Color32,
    /// Unlit switch face.
    face: Color32,
    /// Maker line ("SolPlugs") and model line.
    title: &'static str,
    /// Type line on the plaque.
    model: &'static str,
    /// Knob radius.
    r: f32,
    /// Knob look.
    knob: KnobLook,
    /// What the colour switch is called on this panel.
    colour_name: &'static str,
}

/// Draws the current style's faceplate in `area`. `vu` is the output level in VU
/// (0 VU = -18 dBFS), left and right.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, vu: [f32; 2]) {
    let style = engine.style();
    let mut pl = Plate::new(ui, area);
    match style {
        Style::Pultec => pultec_plate(&mut pl, engine, vu),
        Style::Api => api_plate(&mut pl, engine, vu),
        Style::Ssl => ssl_plate(&mut pl, engine, vu),
        Style::Neve => neve_plate(&mut pl, engine, vu),
        Style::Massive => massive_plate(&mut pl, engine, vu),
        Style::Maag => maag_plate(&mut pl, engine, vu),
        Style::Curve => curve_plate(&mut pl, engine, vu),
        // Clean has its own big graph, never a faceplate.
        Style::Clean => {}
    }
}

/// Case, panel, screws, maker name and plaque.
fn frame(pl: &Plate<'_>, look: &Look, plaque_y: f32) {
    pl.slab(pl.rect, look.case.0, look.case.1, look.brush, 14.0);
    pl.slab(
        pl.r(14.0, 14.0, W - 28.0, H - 28.0),
        look.panel.0,
        look.panel.1,
        look.brush,
        8.0,
    );
    pl.corner_screws(0.0, 0.0, W, H, 30.0, look.metal);
    pl.emboss(
        W * 0.5,
        52.0,
        look.title,
        30.0,
        look.ink,
        Color32::from_black_alpha(120),
    );
    pl.plaque(
        pl.r(W * 0.5 - 150.0, plaque_y, 300.0, 28.0),
        look.model,
        14.0,
        lerp(look.panel.1, Color32::BLACK, 0.35),
        look.metal,
        look.ink,
    );
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    plate::lerp_colour(a, b, t)
}

/// Draws the controls.
fn items(pl: &mut Plate<'_>, e: &Engine, style: Style, look: &Look, list: &[Item]) {
    let knobs = style.knobs();
    for item in list {
        match *item {
            Item::K(i, label, x, y) => {
                let Some(k) = knobs.get(i) else { continue };
                knob(
                    pl,
                    e,
                    Pid::k(style, i),
                    (x, y, look.r),
                    &look.knob,
                    KnobOpts {
                        label,
                        help: k.help,
                        ..Default::default()
                    },
                );
            }
            Item::S(i, caption, x, y) => {
                let Some(k) = knobs.get(i) else { continue };
                let (off, on) = match k.unit {
                    Unit::Switch(a, b) => (a, b),
                    _ => ("Off", "On"),
                };
                switch(pl, e, Pid::k(style, i), (x, y), caption, (off, on), k.help, look);
            }
        }
    }
}

/// A two-way switch drawn as a lamp button with a caption above.
#[allow(clippy::too_many_arguments)]
fn switch(
    pl: &mut Plate<'_>,
    e: &Engine,
    id: Pid,
    (x, y): (f32, f32),
    caption: &str,
    (off, on): (&str, &str),
    help: &str,
    look: &Look,
) {
    let lit = e.params().get(id) >= 0.5;
    if !caption.is_empty() {
        pl.text(x, y - 22.0, caption, 10.5, look.ink);
    }
    let rect = pl.r(x - 38.0, y - 13.0, 76.0, 26.0);
    let resp = plate::button(
        pl,
        ("eq-switch", id.0),
        rect,
        if lit { on } else { off },
        lit,
        look.face,
        look.lit,
        // Switch faces are dark on every panel.
        Color32::from_gray(225),
    )
    .on_hover_text(format!("{caption}: {}\n{help}", if lit { on } else { off }));
    if resp.clicked() {
        e.set(id, if lit { 0.0 } else { 1.0 });
    }
}

/// The colour (harmonics) switch every hardware panel has.
fn colour_switch(pl: &mut Plate<'_>, e: &Engine, style: Style, look: &Look, (x, y): (f32, f32)) {
    switch(
        pl,
        e,
        Pid::c(style, C::Colour),
        (x, y),
        look.colour_name,
        ("Clean", "On"),
        "Harmonic colour from the circuit this style is styled after. Off keeps only the curves.",
        look,
    );
}

/// A row of level LEDs in design rect (x, y, w, h), for `vu` (VU units).
fn led_meter(pl: &Plate<'_>, (x, y, w, h): (f32, f32, f32, f32), vu: f32, ink: Color32) {
    const MARKS: [f32; 14] = [
        -20.0, -15.0, -10.0, -7.0, -5.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 6.0,
    ];
    let back = pl.r(x, y, w, h);
    pl.painter.rect_filled(
        back.expand(pl.s(4.0)),
        CornerRadius::same(pl.s(4.0) as u8),
        Color32::from_rgb(0x12, 0x12, 0x13),
    );
    let n = MARKS.len() as f32;
    let seg = w / n;
    for (i, m) in MARKS.iter().enumerate() {
        let on = vu >= *m;
        let c = match *m {
            v if v > 2.0 => Color32::from_rgb(0xFF, 0x3B, 0x30),
            v if v >= 0.0 => Color32::from_rgb(0xFF, 0xB0, 0x20),
            _ => Color32::from_rgb(0x5D, 0xD6, 0x4B),
        };
        let led = pl.r(x + i as f32 * seg + 2.0, y + 2.0, seg - 4.0, h * 0.55);
        pl.painter.rect_filled(
            led,
            CornerRadius::same(2),
            if on { c } else { lerp(c, Color32::BLACK, 0.8) },
        );
        if i % 2 == 0 || *m == 0.0 {
            pl.text_at(
                x + i as f32 * seg + seg * 0.5,
                y + h * 0.8,
                Align2::CENTER_CENTER,
                &format!("{m:+.0}").replace("+0", "0"),
                pl.font(9.5),
                ink,
            );
        }
    }
}

/// A thin divider line in design coordinates.
fn rule(pl: &Plate<'_>, (x0, y0): (f32, f32), (x1, y1): (f32, f32), c: Color32) {
    pl.painter
        .line_segment([pl.p(x0, y0), pl.p(x1, y1)], Stroke::new(pl.s(1.2).max(1.0), c));
}

/// A boxed section with a title, in design rect.
fn section(pl: &Plate<'_>, (x, y, w, h): (f32, f32, f32, f32), title: &str, c: Color32) {
    let r = pl.r(x, y, w, h);
    pl.painter.rect_stroke(
        r,
        CornerRadius::same(pl.s(6.0) as u8),
        Stroke::new(pl.s(1.2).max(1.0), c),
        StrokeKind::Inside,
    );
    if !title.is_empty() {
        pl.bold(x + w * 0.5, y + 14.0, title, 11.5, c);
    }
}

// ------------------------------------------------------------------ Pultec: cream, valve

fn pultec_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0x2A, 0x22, 0x1A);
    let look = Look {
        case: (Color32::from_rgb(0x9A, 0x94, 0x86), Color32::from_rgb(0x6E, 0x69, 0x5E)),
        panel: (Color32::from_rgb(0xE6, 0xDD, 0xC6), Color32::from_rgb(0xCF, 0xC4, 0xA8)),
        brush: 8,
        metal: Color32::from_rgb(0x9E, 0x98, 0x8A),
        ink,
        lit: Color32::from_rgb(0xFF, 0x9E, 0x3D),
        face: Color32::from_rgb(0x3A, 0x33, 0x2C),
        title: "SolPlugs",
        model: "PROGRAM  EQUALIZER  ·  VALVE",
        r: 30.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x1E, 0x1A, 0x17),
            cap: Color32::from_rgb(0x2C, 0x27, 0x22),
            shine: Color32::from_white_alpha(30),
            pointer: Color32::from_rgb(0xF0, 0xE8, 0xD8),
            ink,
            ridges: 0,
            ticks: true,
        },
        colour_name: "TUBE DRIVE",
    };
    frame(pl, &look, 78.0);
    vu_meter(
        pl,
        (190.0, 116.0, 160.0, 86.0),
        vu_pos(vu[0].max(vu[1])),
        Color32::from_rgb(0xF4, 0xE3, 0xB8),
        ink,
        "OUTPUT",
    );
    section(pl, (36.0, 216.0, 468.0, 134.0), "LOW FREQUENCY", ink);
    section(pl, (36.0, 360.0, 468.0, 140.0), "HIGH FREQUENCY", ink);
    use pultec::*;
    items(
        pl,
        e,
        Style::Pultec,
        &look,
        &[
            Item::K(LOW_BOOST, "BOOST", 120.0, 276.0),
            Item::K(LOW_FREQ, "CPS", 270.0, 276.0),
            Item::K(LOW_ATTEN, "ATTEN", 420.0, 276.0),
            Item::K(HIGH_FREQ, "KCS", 90.0, 424.0),
            Item::K(HIGH_BOOST, "BOOST", 200.0, 424.0),
            Item::K(BANDWIDTH, "BANDWIDTH", 310.0, 424.0),
            Item::K(HIGH_ATTEN, "ATTEN", 420.0, 424.0),
        ],
    );
    items(
        pl,
        e,
        Style::Pultec,
        &look,
        &[Item::K(ATTEN_FREQ, "ATTEN SEL", 150.0, 546.0)],
    );
    colour_switch(pl, e, Style::Pultec, &look, (390.0, 556.0));
}

// ------------------------------------------------------------------ API: grey, 3 bands

fn api_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0xEC, 0xEE, 0xF0);
    let look = Look {
        case: (Color32::from_rgb(0x5A, 0x5E, 0x63), Color32::from_rgb(0x3A, 0x3D, 0x41)),
        panel: (Color32::from_rgb(0x7C, 0x81, 0x87), Color32::from_rgb(0x5E, 0x63, 0x69)),
        brush: 22,
        metal: Color32::from_rgb(0xB0, 0xB4, 0xB8),
        ink,
        lit: Color32::from_rgb(0x6F, 0xD8, 0x6A),
        face: Color32::from_rgb(0x2B, 0x2D, 0x30),
        title: "SolPlugs",
        model: "3-BAND  CONSOLE  EQ  ·  DISCRETE",
        r: 30.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x1A, 0x1B, 0x1D),
            cap: Color32::from_rgb(0x2E, 0x30, 0x33),
            shine: Color32::from_white_alpha(26),
            pointer: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            ink,
            ridges: 0,
            ticks: true,
        },
        colour_name: "DRIVE",
    };
    frame(pl, &look, 78.0);
    use api::*;
    for (i, t) in ["LOW", "MID", "HIGH"].iter().enumerate() {
        section(pl, (40.0 + i as f32 * 156.0, 124.0, 148.0, 352.0), t, ink);
    }
    items(
        pl,
        e,
        Style::Api,
        &look,
        &[
            Item::K(LOW_GAIN, "GAIN", 114.0, 196.0),
            Item::K(LOW_FREQ, "HZ", 114.0, 318.0),
            Item::S(LOW_SHELF, "", 114.0, 440.0),
            Item::K(MID_GAIN, "GAIN", 270.0, 196.0),
            Item::K(MID_FREQ, "HZ", 270.0, 318.0),
            Item::K(HIGH_GAIN, "GAIN", 426.0, 196.0),
            Item::K(HIGH_FREQ, "HZ", 426.0, 318.0),
            Item::S(HIGH_SHELF, "", 426.0, 440.0),
            Item::S(FILTER, "BAND-PASS", 270.0, 440.0),
        ],
    );
    led_meter(pl, (60.0, 510.0, 420.0, 34.0), vu[0].max(vu[1]), ink);
    colour_switch(pl, e, Style::Api, &look, (270.0, 590.0));
}

// ------------------------------------------------------------------ SSL: dark, coloured caps

fn ssl_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0xE8, 0xE8, 0xE4);
    let mut look = Look {
        case: (Color32::from_rgb(0x2E, 0x30, 0x33), Color32::from_rgb(0x1C, 0x1D, 0x1F)),
        panel: (Color32::from_rgb(0x3A, 0x3C, 0x40), Color32::from_rgb(0x2A, 0x2C, 0x2F)),
        brush: 10,
        metal: Color32::from_rgb(0x8A, 0x8D, 0x91),
        ink,
        lit: Color32::from_rgb(0xFF, 0x4A, 0x3A),
        face: Color32::from_rgb(0x22, 0x23, 0x25),
        title: "SolPlugs",
        model: "CHANNEL  EQ  ·  4-BAND  CONSOLE",
        r: 22.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x15, 0x15, 0x16),
            cap: Color32::from_rgb(0x2A, 0x2A, 0x2C),
            shine: Color32::from_white_alpha(30),
            pointer: Color32::WHITE,
            ink,
            ridges: 18,
            ticks: true,
        },
        colour_name: "DRIVE",
    };
    frame(pl, &look, 78.0);
    use ssl::*;
    // Each band gets its cap colour, like the console's.
    let caps = [
        ("LF", Color32::from_rgb(0x2A, 0x2A, 0x2C)),
        ("LMF", Color32::from_rgb(0x2B, 0x5F, 0xB8)),
        ("HMF", Color32::from_rgb(0x2E, 0x8C, 0x4A)),
        ("HF", Color32::from_rgb(0xB8, 0x2E, 0x2E)),
    ];
    let cols = [
        [Item::K(LF_GAIN, "GAIN", 0.0, 0.0), Item::K(LF_FREQ, "HZ", 0.0, 0.0), Item::S(LF_BELL, "", 0.0, 0.0)],
        [Item::K(LMF_GAIN, "GAIN", 0.0, 0.0), Item::K(LMF_FREQ, "HZ", 0.0, 0.0), Item::K(LMF_Q, "Q", 0.0, 0.0)],
        [Item::K(HMF_GAIN, "GAIN", 0.0, 0.0), Item::K(HMF_FREQ, "HZ", 0.0, 0.0), Item::K(HMF_Q, "Q", 0.0, 0.0)],
        [Item::K(HF_GAIN, "GAIN", 0.0, 0.0), Item::K(HF_FREQ, "HZ", 0.0, 0.0), Item::S(HF_BELL, "", 0.0, 0.0)],
    ];
    for (c, ((name, cap), col)) in caps.iter().zip(cols).enumerate() {
        let x = 90.0 + c as f32 * 120.0;
        section(pl, (x - 54.0, 124.0, 108.0, 370.0), name, ink);
        look.knob.cap = *cap;
        let placed: Vec<Item> = col
            .iter()
            .enumerate()
            .map(|(row, it)| {
                let y = 186.0 + row as f32 * 118.0;
                match *it {
                    Item::K(i, l, _, _) => Item::K(i, l, x, y),
                    Item::S(i, l, _, _) => Item::S(i, l, x, y + 10.0),
                }
            })
            .collect();
        items(pl, e, Style::Ssl, &look, &placed);
    }
    look.knob.cap = Color32::from_rgb(0x6C, 0x6E, 0x72);
    items(
        pl,
        e,
        Style::Ssl,
        &look,
        &[
            Item::K(HPF, "HIGH PASS", 90.0, 548.0),
            Item::K(LPF, "LOW PASS", 210.0, 548.0),
        ],
    );
    led_meter(pl, (290.0, 516.0, 216.0, 30.0), vu[0].max(vu[1]), ink);
    colour_switch(pl, e, Style::Ssl, &look, (398.0, 590.0));
}

// ------------------------------------------------------------------ Neve: blue-grey, big knobs

fn neve_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0xF0, 0xF0, 0xEA);
    let look = Look {
        case: (Color32::from_rgb(0x4C, 0x55, 0x60), Color32::from_rgb(0x33, 0x3A, 0x43)),
        panel: (Color32::from_rgb(0x6A, 0x78, 0x88), Color32::from_rgb(0x55, 0x62, 0x70)),
        brush: 14,
        metal: Color32::from_rgb(0xA8, 0xAE, 0xB4),
        ink,
        lit: Color32::from_rgb(0xFF, 0x3E, 0x32),
        face: Color32::from_rgb(0x2A, 0x2F, 0x35),
        title: "SolPlugs",
        model: "CLASS-A  CONSOLE  EQ  ·  TRANSFORMER",
        r: 34.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x8A, 0x2A, 0x24),
            cap: Color32::from_rgb(0x9E, 0x34, 0x2C),
            shine: Color32::from_white_alpha(34),
            pointer: Color32::WHITE,
            ink,
            ridges: 0,
            ticks: true,
        },
        colour_name: "IRON",
    };
    frame(pl, &look, 78.0);
    vu_meter(
        pl,
        (190.0, 118.0, 160.0, 92.0),
        vu_pos(vu[0].max(vu[1])),
        Color32::from_rgb(0xF2, 0xE6, 0xC4),
        Color32::from_rgb(0x22, 0x1E, 0x18),
        "",
    );
    use neve::*;
    let mut grey = look.knob;
    grey.body = Color32::from_rgb(0x3E, 0x44, 0x4C);
    grey.cap = Color32::from_rgb(0x52, 0x5A, 0x64);
    let grey_look = Look { knob: grey, ..look };
    // Frequencies on grey knobs, gains on red, like the console.
    items(
        pl,
        e,
        Style::Neve,
        &grey_look,
        &[
            Item::K(HPF, "HIGH PASS", 110.0, 290.0),
            Item::K(LOW_FREQ, "LOW HZ", 270.0, 290.0),
            Item::K(MID_FREQ, "MID HZ", 430.0, 290.0),
        ],
    );
    items(
        pl,
        e,
        Style::Neve,
        &look,
        &[
            Item::K(HIGH_GAIN, "HIGH 12k", 110.0, 440.0),
            Item::K(LOW_GAIN, "LOW", 270.0, 440.0),
            Item::K(MID_GAIN, "MID", 430.0, 440.0),
        ],
    );
    rule(pl, (40.0, 384.0), (500.0, 384.0), ink.gamma_multiply(0.4));
    colour_switch(pl, e, Style::Neve, &look, (270.0, 586.0));
}

// ------------------------------------------------------------------ Massive: cream, 4 channels

fn massive_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0x26, 0x24, 0x20);
    let look = Look {
        case: (Color32::from_rgb(0x8E, 0x8A, 0x80), Color32::from_rgb(0x66, 0x62, 0x5A)),
        panel: (Color32::from_rgb(0xE8, 0xE2, 0xD2), Color32::from_rgb(0xD2, 0xCA, 0xB6)),
        brush: 6,
        metal: Color32::from_rgb(0x9C, 0x98, 0x8E),
        ink,
        lit: Color32::from_rgb(0xFF, 0xB4, 0x3C),
        face: Color32::from_rgb(0x3A, 0x36, 0x30),
        title: "SolPlugs",
        model: "PASSIVE  MASTERING  EQ  ·  4-BAND",
        r: 18.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x24, 0x22, 0x20),
            cap: Color32::from_rgb(0x38, 0x35, 0x31),
            shine: Color32::from_white_alpha(28),
            pointer: Color32::from_rgb(0xF4, 0xEE, 0xE0),
            ink,
            ridges: 0,
            ticks: true,
        },
        colour_name: "VALVE",
    };
    frame(pl, &look, 72.0);
    let face = Color32::from_rgb(0xF4, 0xE3, 0xB8);
    vu_meter(pl, (120.0, 112.0, 130.0, 72.0), vu_pos(vu[0]), face, ink, "L");
    vu_meter(pl, (290.0, 112.0, 130.0, 72.0), vu_pos(vu[1]), face, ink, "R");
    for b in 0..4 {
        let x = 90.0 + b as f32 * 120.0;
        section(pl, (x - 54.0, 196.0, 108.0, 324.0), ["1", "2", "3", "4"][b], ink);
        items(
            pl,
            e,
            Style::Massive,
            &look,
            &[
                Item::K(massive::gain(b), "GAIN", x, 240.0),
                Item::K(massive::freq(b), "FREQ", x, 324.0),
                Item::K(massive::bandwidth(b), "WIDTH", x, 408.0),
                Item::S(massive::shelf(b), "", x, 494.0),
            ],
        );
    }
    items(
        pl,
        e,
        Style::Massive,
        &look,
        &[
            Item::K(massive::HPF, "HIGH PASS", 90.0, 560.0),
            Item::K(massive::LPF, "LOW PASS", 450.0, 560.0),
        ],
    );
    colour_switch(pl, e, Style::Massive, &look, (270.0, 578.0));
}

// ------------------------------------------------------------------ Maag: black, silver, air

fn maag_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0xE6, 0xE8, 0xEA);
    let look = Look {
        case: (Color32::from_rgb(0x26, 0x27, 0x29), Color32::from_rgb(0x12, 0x12, 0x13)),
        panel: (Color32::from_rgb(0x1E, 0x1F, 0x21), Color32::from_rgb(0x15, 0x15, 0x16)),
        brush: 4,
        metal: Color32::from_rgb(0x9A, 0x9E, 0xA2),
        ink,
        lit: Color32::from_rgb(0x5A, 0xB8, 0xFF),
        face: Color32::from_rgb(0x2C, 0x2D, 0x30),
        title: "SolPlugs",
        model: "AIR  BAND  EQ  ·  OPEN  TOP",
        r: 32.0,
        knob: KnobLook {
            body: Color32::from_rgb(0xA8, 0xAC, 0xB0),
            cap: Color32::from_rgb(0xC8, 0xCC, 0xD0),
            shine: Color32::from_white_alpha(70),
            pointer: Color32::from_rgb(0x20, 0x20, 0x22),
            ink,
            ridges: 36,
            ticks: true,
        },
        colour_name: "COLOUR",
    };
    frame(pl, &look, 78.0);
    led_meter(pl, (70.0, 128.0, 400.0, 32.0), vu[0].max(vu[1]), ink);
    use maag::*;
    let mut air = look.knob;
    air.cap = Color32::from_rgb(0x4A, 0x9E, 0xE6);
    air.body = Color32::from_rgb(0x36, 0x7C, 0xBE);
    air.pointer = Color32::WHITE;
    let air_look = Look { knob: air, ..look };
    items(
        pl,
        e,
        Style::Maag,
        &look,
        &[
            Item::K(LOW_GAIN, "LOW", 110.0, 250.0),
            Item::K(MID_GAIN, "MID", 270.0, 250.0),
            Item::K(LOW_FREQ, "LOW HZ", 110.0, 390.0),
            Item::K(MID_FREQ, "MID HZ", 270.0, 390.0),
            Item::S(HPF, "SUB CUT", 110.0, 540.0),
        ],
    );
    items(
        pl,
        e,
        Style::Maag,
        &air_look,
        &[
            Item::K(AIR_GAIN, "AIR", 430.0, 250.0),
            Item::K(AIR_FREQ, "AIR HZ", 430.0, 390.0),
        ],
    );
    colour_switch(pl, e, Style::Maag, &look, (330.0, 540.0));
}

// ------------------------------------------------------------------ Curve: grey, valve console

fn curve_plate(pl: &mut Plate<'_>, e: &Engine, vu: [f32; 2]) {
    let ink = Color32::from_rgb(0x1E, 0x20, 0x22);
    let look = Look {
        case: (Color32::from_rgb(0x70, 0x72, 0x74), Color32::from_rgb(0x4E, 0x50, 0x52)),
        panel: (Color32::from_rgb(0xB4, 0xB6, 0xB4), Color32::from_rgb(0x98, 0x9A, 0x98)),
        brush: 16,
        metal: Color32::from_rgb(0xC4, 0xC6, 0xC8),
        ink,
        lit: Color32::from_rgb(0xFF, 0x8A, 0x2A),
        face: Color32::from_rgb(0x3A, 0x3C, 0x3E),
        title: "SolPlugs",
        model: "VALVE  CONSOLE  EQ  ·  BROADCAST",
        r: 26.0,
        knob: KnobLook {
            body: Color32::from_rgb(0x20, 0x20, 0x22),
            cap: Color32::from_rgb(0x34, 0x34, 0x36),
            shine: Color32::from_white_alpha(28),
            pointer: Color32::from_rgb(0xF2, 0xF2, 0xEE),
            ink,
            ridges: 0,
            ticks: true,
        },
        colour_name: "VALVE",
    };
    frame(pl, &look, 78.0);
    vu_meter(
        pl,
        (190.0, 118.0, 160.0, 92.0),
        vu_pos(vu[0].max(vu[1])),
        Color32::from_rgb(0xF2, 0xE8, 0xCE),
        ink,
        "",
    );
    use curve::*;
    for (i, t) in ["BASS", "MID", "TREBLE"].iter().enumerate() {
        section(pl, (40.0 + i as f32 * 156.0, 222.0, 148.0, 286.0), t, ink);
    }
    items(
        pl,
        e,
        Style::Curve,
        &look,
        &[
            Item::K(BASS_GAIN, "GAIN", 114.0, 284.0),
            Item::K(BASS_FREQ, "HZ", 114.0, 388.0),
            Item::S(BASS_SHELF, "", 114.0, 486.0),
            Item::K(MID_GAIN, "GAIN", 270.0, 284.0),
            Item::K(MID_FREQ, "HZ", 270.0, 388.0),
            Item::K(TREBLE_GAIN, "GAIN", 426.0, 284.0),
            Item::K(TREBLE_FREQ, "HZ", 426.0, 388.0),
            Item::S(TREBLE_SHELF, "", 426.0, 486.0),
            Item::K(HPF, "HIGH PASS", 150.0, 548.0),
        ],
    );
    colour_switch(pl, e, Style::Curve, &look, (390.0, 560.0));
}
