//! The five hardware faceplates: playful SolPlugs takes on the classic layouts, no copied
//! logos or names. Every plate carries every knob (Threshold, Ratio, Attack, Release,
//! Knee, Gain In, Gain Out/Makeup, Mix, Wet, Dry), arranged to suit the plate, plus the
//! unit's own meter.

use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use solplugs_comp_core::Mode;

use super::plate::{
    self, H, KnobLook, KnobOpts, Plate, W, button, gr_to_needle, knob, lamp, needle_meter,
};
use crate::engine::Engine;
use crate::params::{G, K, Pid};

/// What the meters show this frame (smoothed in the UI).
#[derive(Clone, Copy, Debug, Default)]
pub struct MeterView {
    /// Needle positions (0..1, 1 = rest), left and right.
    pub needle: [f32; 2],
    /// Gain reduction for bar meters, dB.
    pub gr: f32,
}

/// Version shown on the plates.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Help text for a knob.
pub fn help(k: K) -> &'static str {
    match k {
        K::Threshold => "Level where compression starts. Lower = more compression.",
        K::Ratio => "How hard it pushes down above the threshold. 4:1 = 4 dB in, 1 dB out.",
        K::Attack => "How fast it grabs a peak.",
        K::Release => "How fast it lets go after the peak.",
        K::Knee => "How gently it eases into compression around the threshold.",
        K::GainIn => "Level into the compressor. More in = more compression (and more colour).",
        K::GainOut => {
            "Final output level: the makeup gain. MATCH and auto-makeup set it. Touching it turns auto-makeup off."
        }
        K::Mix => "Blend of compressed (wet) and untouched (dry) signal: parallel compression.",
        K::Wet => "Level of the compressed part.",
        K::Dry => "Level of the untouched part.",
        K::Lookahead => "Clean only: sees peaks coming so none slip through. Adds latency.",
        _ => "",
    }
}

fn opts(k: K, label: &'static str) -> KnobOpts<'static> {
    KnobOpts {
        label,
        help: help(k),
        ..Default::default()
    }
}

/// Gain Out knob options: shows the live auto-makeup gain while it's on.
fn gain_out_opts(engine: &Engine, mode: Mode, label: &'static str) -> KnobOpts<'static> {
    let auto = engine.params().m(mode, K::AutoMakeup) >= 0.5;
    KnobOpts {
        label,
        hint: if auto { "AUTO" } else { "" },
        value_override: auto.then(|| engine.meters().auto_makeup()),
        help: help(K::GainOut),
        ..Default::default()
    }
}

/// Draws the faceplate for the active mode into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, meters: &MeterView) {
    let mode = engine.mode();
    let mut plate = Plate::new(ui, area);
    match mode {
        Mode::Vca => vca(&mut plate, engine, meters),
        Mode::Opto => opto(&mut plate, engine, meters),
        Mode::Fet => fet(&mut plate, engine, meters),
        Mode::VariMu => varimu(&mut plate, engine, meters),
        Mode::Clean => clean(&mut plate, engine, meters),
    }
}

// ---------------------------------------------------------------- VCA: dark 80s bus comp

fn vca(pl: &mut Plate<'_>, e: &Engine, m: &MeterView) {
    let id = |k| Pid::m(Mode::Vca, k);
    let steel = Color32::from_rgb(0x8C, 0x90, 0x94);
    let ink = Color32::from_rgb(0xE4, 0xE6, 0xE8);
    pl.slab(
        pl.rect,
        Color32::from_rgb(0x4A, 0x4E, 0x53),
        Color32::from_rgb(0x2B, 0x2E, 0x32),
        26,
        14.0,
    );
    pl.slab(
        pl.r(14.0, 14.0, W - 28.0, H - 28.0),
        Color32::from_rgb(0x44, 0x48, 0x4D),
        Color32::from_rgb(0x30, 0x33, 0x37),
        18,
        8.0,
    );
    pl.corner_screws(0.0, 0.0, W, H, 30.0, steel);
    // Logo.
    pl.sun(150.0, 78.0, 34.0, Color32::from_rgb(0xD6, 0xD9, 0xDC));
    pl.emboss(
        330.0,
        78.0,
        "SolPlugs",
        54.0,
        Color32::from_rgb(0xD0, 0xD3, 0xD6),
        Color32::from_black_alpha(160),
    );
    pl.plaque(
        pl.r(160.0, 128.0, 330.0, 36.0),
        "VCA  BUS  COMPRESSOR",
        17.0,
        Color32::from_rgb(0x2A, 0x2D, 0x31),
        steel,
        ink,
    );
    // LED gain-reduction column.
    let col = pl.r(40.0, 120.0, 70.0, 410.0);
    pl.painter.rect_filled(
        col,
        CornerRadius::same(pl.s(6.0) as u8),
        Color32::from_rgb(0x1A, 0x1C, 0x1F),
    );
    pl.painter.rect_stroke(
        col,
        CornerRadius::same(pl.s(6.0) as u8),
        Stroke::new(1.5, steel),
        StrokeKind::Inside,
    );
    let steps = [
        20.0, 14.0, 10.0, 8.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.5, 1.0, 0.5,
    ];
    for (i, db) in steps.iter().enumerate() {
        let y = 140.0 + i as f32 * 30.0;
        let on = m.gr >= *db;
        let c = match *db {
            d if d >= 10.0 => Color32::from_rgb(0xFF, 0x3B, 0x30),
            d if d >= 4.0 => Color32::from_rgb(0xFF, 0xB0, 0x20),
            _ => Color32::from_rgb(0x5D, 0xD6, 0x4B),
        };
        let led = pl.r(68.0, y, 32.0, 14.0);
        if on {
            pl.painter.rect_filled(
                led.expand(pl.s(3.0)),
                CornerRadius::same(3),
                c.gamma_multiply(0.25),
            );
        }
        pl.painter.rect_filled(
            led,
            CornerRadius::same(2),
            if on {
                c
            } else {
                plate::lerp_colour(c, Color32::BLACK, 0.8)
            },
        );
        pl.text_at(
            62.0,
            y + 7.0,
            Align2::RIGHT_CENTER,
            &format!("{db}"),
            pl.font(10.5),
            ink,
        );
    }
    pl.text(75.0, 548.0, "GAIN", 11.0, ink);
    pl.text(75.0, 562.0, "REDUCTION", 11.0, ink);
    // Main row.
    let look = KnobLook {
        body: Color32::from_rgb(0x26, 0x28, 0x2B),
        cap: Color32::from_rgb(0x3A, 0x3D, 0x41),
        shine: Color32::from_white_alpha(28),
        pointer: Color32::from_rgb(0xF2, 0xF2, 0xF2),
        ink,
        ridges: 28,
        ticks: true,
    };
    let row1 = [
        (K::Threshold, "Threshold"),
        (K::Ratio, "Ratio"),
        (K::Attack, "Attack"),
        (K::Release, "Release"),
    ];
    for (i, (k, label)) in row1.into_iter().enumerate() {
        knob(
            pl,
            e,
            id(k),
            (162.0 + i as f32 * 80.0, 232.0, 28.0),
            &look,
            opts(k, label),
        );
    }
    knob(
        pl,
        e,
        id(K::GainOut),
        (162.0 + 4.0 * 80.0, 232.0, 28.0),
        &look,
        gain_out_opts(e, Mode::Vca, "Makeup"),
    );
    let small = KnobLook { ridges: 18, ..look };
    let row2 = [
        (K::GainIn, "Gain In"),
        (K::Knee, "Knee"),
        (K::Mix, "Mix"),
        (K::Wet, "Wet"),
        (K::Dry, "Dry"),
    ];
    for (i, (k, label)) in row2.into_iter().enumerate() {
        knob(
            pl,
            e,
            id(k),
            (162.0 + i as f32 * 80.0, 380.0, 22.0),
            &small,
            opts(k, label),
        );
    }
    // IN / BYPASS and AUTO release.
    let bypass = e.params().on(Pid::g(G::Bypass));
    let face = Color32::from_rgb(0x22, 0x24, 0x27);
    if button(
        pl,
        ("vca-in", 0),
        pl.r(160.0, 490.0, 88.0, 38.0),
        "IN",
        !bypass,
        face,
        Color32::from_rgb(0x5D, 0xD6, 0x4B),
        ink,
    )
    .clicked()
    {
        e.set(Pid::g(G::Bypass), 0.0);
    }
    if button(
        pl,
        ("vca-byp", 0),
        pl.r(262.0, 490.0, 110.0, 38.0),
        "BYPASS",
        bypass,
        face,
        Color32::from_rgb(0xFF, 0xB0, 0x20),
        ink,
    )
    .clicked()
    {
        e.set(Pid::g(G::Bypass), 1.0);
    }
    let auto = e.params().on(id(K::AutoRelease));
    if button(
        pl,
        ("vca-auto", 0),
        pl.r(386.0, 490.0, 110.0, 38.0),
        "AUTO REL",
        auto,
        face,
        Color32::from_rgb(0x5D, 0xD6, 0x4B),
        ink,
    )
    .on_hover_text("Auto release: lets transients go fast and sustained compression go slowly.")
    .clicked()
    {
        e.set(id(K::AutoRelease), if auto { 0.0 } else { 1.0 });
    }
    pl.plaque(
        pl.r(130.0, 572.0, 370.0, 30.0),
        &format!("SolPlugs  ·  VCA-BUS  ·  v{VERSION}"),
        12.5,
        Color32::from_rgb(0x2A, 0x2D, 0x31),
        steel,
        ink,
    );
}

// ---------------------------------------------------------------- Opto: silver 60s leveller

fn opto(pl: &mut Plate<'_>, e: &Engine, m: &MeterView) {
    let id = |k| Pid::m(Mode::Opto, k);
    let dark = Color32::from_rgb(0x2E, 0x31, 0x35);
    pl.painter.rect_filled(
        pl.rect,
        CornerRadius::same(pl.s(10.0) as u8),
        Color32::from_rgb(0x1E, 0x20, 0x23),
    );
    pl.slab(
        pl.r(10.0, 10.0, W - 20.0, H - 20.0),
        Color32::from_rgb(0xC9, 0xCC, 0xD0),
        Color32::from_rgb(0x8F, 0x93, 0x98),
        34,
        22.0,
    );
    pl.corner_screws(
        10.0,
        10.0,
        W - 20.0,
        H - 20.0,
        26.0,
        Color32::from_rgb(0xA8, 0xAC, 0xB0),
    );
    pl.sun(270.0, 52.0, 26.0, dark);
    pl.emboss(
        270.0,
        104.0,
        "SolPlugs",
        44.0,
        dark,
        Color32::from_white_alpha(140),
    );
    pl.plaque(
        pl.r(150.0, 132.0, 240.0, 28.0),
        "OPTICAL  TUBE  COMPRESSOR",
        13.0,
        Color32::from_rgb(0xB4, 0xB8, 0xBC),
        dark,
        dark,
    );
    // Round copper meter.
    let (mx, my, mr) = (270.0, 270.0, 92.0);
    let c = pl.p(mx, my);
    pl.painter.circle_filled(
        c + vec2(0.0, pl.s(3.0)),
        pl.s(mr + 14.0),
        Color32::from_black_alpha(90),
    );
    pl.painter
        .circle_filled(c, pl.s(mr + 12.0), Color32::from_rgb(0xB8, 0x74, 0x3A));
    pl.painter
        .circle_filled(c, pl.s(mr + 5.0), Color32::from_rgb(0x6A, 0x3E, 0x1E));
    pl.painter
        .circle_filled(c, pl.s(mr), Color32::from_rgb(0x1C, 0x15, 0x10));
    pl.painter.circle_filled(
        c + vec2(0.0, pl.s(10.0)),
        pl.s(mr * 0.8),
        Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x30, 28),
    );
    let amber = Color32::from_rgb(0xF5, 0xC0, 0x7A);
    needle_meter(
        pl,
        (mx, my + 28.0),
        mr * 0.82,
        (-2.45, -0.69),
        m.needle[0],
        amber,
        Color32::from_rgb(0xFF, 0xE8, 0xC8),
    );
    pl.text(mx, my + 52.0, "GAIN REDUCTION (dB)", 10.5, amber);
    // Bypass toggle with lamps (left of the meter).
    let bypass = e.params().on(Pid::g(G::Bypass));
    lamp(
        pl,
        70.0,
        230.0,
        7.0,
        !bypass,
        Color32::from_rgb(0x7C, 0xE0, 0x6A),
    );
    lamp(
        pl,
        70.0,
        256.0,
        7.0,
        bypass,
        Color32::from_rgb(0xFF, 0xA0, 0x30),
    );
    let sw = pl.r(54.0, 276.0, 32.0, 52.0);
    let resp = pl
        .ui
        .interact(sw, pl.ui.scope_id().with("opto-byp"), egui::Sense::click())
        .on_hover_text("Bypass");
    pl.painter.rect_filled(
        sw,
        CornerRadius::same(pl.s(6.0) as u8),
        Color32::from_rgb(0x55, 0x58, 0x5C),
    );
    let lever_y = if bypass {
        sw.bottom() - pl.s(14.0)
    } else {
        sw.top() + pl.s(14.0)
    };
    pl.painter.line_segment(
        [sw.center(), pos2(sw.center().x, lever_y)],
        Stroke::new(pl.s(7.0), Color32::from_rgb(0xDD, 0xDF, 0xE2)),
    );
    pl.painter.circle_filled(
        pos2(sw.center().x, lever_y),
        pl.s(6.0),
        Color32::from_rgb(0xF0, 0xF1, 0xF2),
    );
    pl.text(
        70.0,
        342.0,
        if bypass { "BYPASS" } else { "ACTIVE" },
        11.0,
        dark,
    );
    if resp.clicked() {
        e.set(Pid::g(G::Bypass), if bypass { 0.0 } else { 1.0 });
    }
    // Detector / limit-ish switch on the right: RMS/peak.
    let rms = e.params().on(id(K::Detector));
    if button(
        pl,
        ("opto-det", 0),
        pl.r(436.0, 240.0, 70.0, 30.0),
        if rms { "RMS" } else { "PEAK" },
        false,
        Color32::from_rgb(0x70, 0x74, 0x79),
        Color32::WHITE,
        Color32::WHITE,
    )
    .on_hover_text("Detector: RMS (smooth, like the light cell) or peak.")
    .clicked()
    {
        e.set(id(K::Detector), if rms { 0.0 } else { 1.0 });
    }
    pl.text(471.0, 284.0, "DETECTOR", 10.0, dark);
    // Two big chrome knobs.
    let chrome = KnobLook {
        body: Color32::from_rgb(0xB9, 0xBD, 0xC2),
        cap: Color32::from_rgb(0xD8, 0xDB, 0xDE),
        shine: Color32::from_white_alpha(110),
        pointer: Color32::from_rgb(0x30, 0x32, 0x36),
        ink: dark,
        ridges: 36,
        ticks: false,
    };
    knob(
        pl,
        e,
        id(K::GainOut),
        (110.0, 428.0, 40.0),
        &chrome,
        gain_out_opts(e, Mode::Opto, "Gain"),
    );
    knob(
        pl,
        e,
        id(K::Threshold),
        (430.0, 428.0, 40.0),
        &chrome,
        KnobOpts {
            reversed: true,
            label: "Peak Reduction",
            help: "How much it levels: turn up for more (lowers the threshold).",
            ..Default::default()
        },
    );
    // Small knobs: a 2x2 block between the big ones, then a row along the bottom.
    let small = KnobLook {
        ridges: 16,
        ticks: true,
        ..chrome
    };
    let block = [
        (K::GainIn, "In"),
        (K::Ratio, "Ratio"),
        (K::Attack, "Attack"),
        (K::Release, "Release"),
    ];
    for (i, (k, label)) in block.into_iter().enumerate() {
        let (x, y) = (225.0 + (i % 2) as f32 * 90.0, 404.0 + (i / 2) as f32 * 74.0);
        knob(pl, e, id(k), (x, y, 14.0), &small, opts(k, label));
    }
    let row = [
        (K::Knee, "Knee"),
        (K::Mix, "Mix"),
        (K::Wet, "Wet"),
        (K::Dry, "Dry"),
    ];
    for (i, (k, label)) in row.into_iter().enumerate() {
        knob(
            pl,
            e,
            id(k),
            (120.0 + i as f32 * 100.0, 552.0, 15.0),
            &small,
            opts(k, label),
        );
    }
    pl.text_at(
        72.0,
        617.0,
        Align2::LEFT_CENTER,
        "TUBE · OPTO · 1960s STYLE",
        pl.font(10.0),
        dark,
    );
    pl.plaque(
        pl.r(290.0, 606.0, 180.0, 22.0),
        &format!("MODEL SOL-OPTO · v{VERSION}"),
        10.0,
        Color32::from_rgb(0xA4, 0xA8, 0xAD),
        dark,
        dark,
    );
}

// ---------------------------------------------------------------- FET: black 70s FET box

fn fet(pl: &mut Plate<'_>, e: &Engine, m: &MeterView) {
    let id = |k| Pid::m(Mode::Fet, k);
    let gold = Color32::from_rgb(0xE8, 0xC0, 0x6A);
    let ink = Color32::from_rgb(0xE8, 0xDC, 0xC0);
    pl.slab(
        pl.rect,
        Color32::from_rgb(0x5A, 0x5C, 0x60),
        Color32::from_rgb(0x3A, 0x3C, 0x40),
        20,
        18.0,
    );
    pl.slab(
        pl.r(12.0, 12.0, W - 24.0, H - 24.0),
        Color32::from_rgb(0x1E, 0x1F, 0x21),
        Color32::from_rgb(0x11, 0x12, 0x13),
        16,
        12.0,
    );
    pl.corner_screws(
        12.0,
        12.0,
        W - 24.0,
        H - 24.0,
        22.0,
        Color32::from_rgb(0x50, 0x52, 0x55),
    );
    pl.sun(150.0, 66.0, 28.0, gold);
    pl.bold(318.0, 66.0, "SolPlugs", 48.0, gold);
    pl.text(270.0, 112.0, "FET COMPRESSOR  ·  MODEL SP-F76", 14.0, gold);
    pl.painter.line_segment(
        [pl.p(40.0, 132.0), pl.p(500.0, 132.0)],
        Stroke::new(1.0, Color32::from_gray(70)),
    );
    // Ratio buttons + all buttons in.
    let ratio = e.params().m(Mode::Fet, K::Ratio);
    let all = e.params().on(id(K::AllButtons));
    let face = Color32::from_rgb(0x2A, 0x2B, 0x2E);
    let lit = Color32::from_rgb(0xF2, 0xB8, 0x4B);
    for (i, r) in [4.0, 8.0, 12.0, 20.0].into_iter().enumerate() {
        let x = 54.0 + i as f32 * 92.0;
        let on = all || (ratio - r).abs() < 0.5;
        lamp(
            pl,
            x + 38.0,
            152.0,
            6.0,
            on,
            Color32::from_rgb(0xFF, 0x9F, 0x2E),
        );
        if button(
            pl,
            ("fet-ratio", i as u32),
            pl.r(x, 166.0, 76.0, 36.0),
            &format!("{r}:1"),
            on,
            face,
            lit,
            ink,
        )
        .on_hover_text(format!("Ratio {r}:1"))
        .clicked()
        {
            e.set(id(K::AllButtons), 0.0);
            e.set(id(K::Ratio), r);
        }
    }
    if button(pl, ("fet-all", 0), pl.r(422.0, 166.0, 76.0, 36.0), "ALL", all, face, Color32::from_rgb(0xFF, 0x60, 0x3A), ink)
        .on_hover_text("All buttons in: press every ratio button at once for the famous smash. Loud, gritty, the meter pins.")
        .clicked()
    {
        e.set(id(K::AllButtons), if all { 0.0 } else { 1.0 });
    }
    lamp(
        pl,
        460.0,
        152.0,
        6.0,
        all,
        Color32::from_rgb(0xFF, 0x4A, 0x2A),
    );
    // Arc VU meter.
    let meter = pl.r(120.0, 222.0, 300.0, 150.0);
    pl.painter.rect_filled(
        meter.expand(pl.s(8.0)),
        CornerRadius::same(pl.s(12.0) as u8),
        Color32::from_rgb(0x0C, 0x0C, 0x0D),
    );
    pl.painter.rect_filled(
        meter,
        CornerRadius::same(pl.s(8.0) as u8),
        Color32::from_rgb(0x16, 0x1E, 0x18),
    );
    pl.painter.with_clip_rect(meter).circle_filled(
        pl.p(270.0, 372.0),
        pl.s(120.0),
        Color32::from_rgba_unmultiplied(0x9C, 0xE0, 0x80, 14),
    );
    let green = Color32::from_rgb(0xB9, 0xE6, 0x9A);
    needle_meter(
        pl,
        (270.0, 372.0),
        128.0,
        (-2.55, -0.59),
        m.needle[0],
        green,
        Color32::from_rgb(0xF4, 0xE6, 0xC8),
    );
    pl.text(270.0, 352.0, "GAIN REDUCTION · dB", 11.0, green);
    // INPUT / OUTPUT big knobs, threshold + knee between.
    let big = KnobLook {
        body: Color32::from_rgb(0x15, 0x15, 0x16),
        cap: Color32::from_rgb(0x24, 0x24, 0x26),
        shine: Color32::from_white_alpha(18),
        pointer: gold,
        ink,
        ridges: 40,
        ticks: true,
    };
    knob(
        pl,
        e,
        id(K::GainIn),
        (110.0, 424.0, 40.0),
        &big,
        opts(K::GainIn, "INPUT"),
    );
    knob(
        pl,
        e,
        id(K::GainOut),
        (430.0, 424.0, 40.0),
        &big,
        gain_out_opts(e, Mode::Fet, "OUTPUT"),
    );
    let mid = KnobLook { ridges: 20, ..big };
    knob(
        pl,
        e,
        id(K::Threshold),
        (226.0, 418.0, 22.0),
        &mid,
        opts(K::Threshold, "Threshold"),
    );
    knob(
        pl,
        e,
        id(K::Knee),
        (314.0, 418.0, 22.0),
        &mid,
        opts(K::Knee, "Knee"),
    );
    // ATTACK / RELEASE (turn right = faster, as on the classic box).
    knob(
        pl,
        e,
        id(K::Attack),
        (110.0, 552.0, 22.0),
        &mid,
        KnobOpts {
            reversed: true,
            label: "ATTACK",
            hint: "SLOW — FAST",
            help: help(K::Attack),
            ..Default::default()
        },
    );
    knob(
        pl,
        e,
        id(K::Release),
        (430.0, 552.0, 22.0),
        &mid,
        KnobOpts {
            reversed: true,
            label: "RELEASE",
            hint: "SLOW — FAST",
            help: help(K::Release),
            ..Default::default()
        },
    );
    let small = KnobLook { ridges: 14, ..big };
    for (i, (k, label)) in [(K::Mix, "Mix"), (K::Wet, "Wet"), (K::Dry, "Dry")]
        .into_iter()
        .enumerate()
    {
        knob(
            pl,
            e,
            id(k),
            (210.0 + i as f32 * 60.0, 530.0, 17.0),
            &small,
            opts(k, label),
        );
    }
    pl.plaque(
        pl.r(170.0, 600.0, 200.0, 20.0),
        &format!("SolPlugs · FET · v{VERSION}"),
        10.0,
        Color32::from_rgb(0x1A, 0x1A, 0x1B),
        gold.gamma_multiply(0.6),
        gold,
    );
}

// ---------------------------------------------------------------- Vari-Mu: cream 50s tube unit

fn varimu(pl: &mut Plate<'_>, e: &Engine, m: &MeterView) {
    let id = |k| Pid::m(Mode::VariMu, k);
    let brown = Color32::from_rgb(0x6A, 0x45, 0x22);
    let ink = Color32::from_rgb(0x3A, 0x2C, 0x1E);
    let brass = Color32::from_rgb(0xB0, 0x8A, 0x4A);
    pl.painter.rect_filled(
        pl.rect,
        CornerRadius::same(pl.s(16.0) as u8),
        Color32::from_rgb(0x2A, 0x24, 0x1C),
    );
    pl.slab(
        pl.r(8.0, 8.0, W - 16.0, H - 16.0),
        Color32::from_rgb(0xF1, 0xEA, 0xD4),
        Color32::from_rgb(0xDD, 0xD2, 0xB2),
        10,
        20.0,
    );
    pl.corner_screws(8.0, 8.0, W - 16.0, H - 16.0, 26.0, brass);
    pl.sun(150.0, 70.0, 30.0, brown);
    pl.bold(320.0, 70.0, "SolPlugs", 52.0, brown);
    pl.painter.line_segment(
        [pl.p(110.0, 118.0), pl.p(430.0, 118.0)],
        Stroke::new(pl.s(2.0), ink),
    );
    pl.text(270.0, 134.0, "VARIABLE-MU TUBE LIMITER", 12.0, ink);
    // Twin VU dials: gain reduction left and right.
    let link = e.params().on(Pid::g(G::Link));
    for (i, label) in ["L", "R"].into_iter().enumerate() {
        let x = 150.0 + i as f32 * 240.0;
        let c = pl.p(x, 262.0);
        pl.painter.circle_filled(
            c + vec2(0.0, pl.s(4.0)),
            pl.s(100.0),
            Color32::from_black_alpha(80),
        );
        pl.painter
            .circle_filled(c, pl.s(98.0), Color32::from_rgb(0x1B, 0x1A, 0x18));
        pl.painter
            .circle_filled(c, pl.s(86.0), Color32::from_rgb(0xF4, 0xEC, 0xD0));
        pl.painter.circle_filled(
            c + vec2(0.0, pl.s(34.0)),
            pl.s(11.0),
            Color32::from_rgb(0x22, 0x20, 0x1D),
        );
        needle_meter(
            pl,
            (x, 296.0),
            72.0,
            (-2.5, -0.64),
            m.needle[i],
            ink,
            Color32::from_rgb(0x1A, 0x16, 0x12),
        );
        pl.text(x, 326.0, "GR", 12.0, ink);
        pl.bold(x, 374.0, label, 15.0, brown);
    }
    pl.text(
        270.0,
        374.0,
        if link { "LINKED" } else { "DUAL" },
        10.0,
        ink,
    );
    let look = KnobLook {
        body: Color32::from_rgb(0x14, 0x12, 0x10),
        cap: Color32::from_rgb(0x26, 0x22, 0x1E),
        shine: Color32::from_white_alpha(22),
        pointer: Color32::from_rgb(0xE0, 0xB8, 0x6A),
        ink,
        ridges: 0,
        ticks: true,
    };
    let row1 = [
        (K::Threshold, "Threshold"),
        (K::Attack, "Attack"),
        (K::Release, "Release"),
        (K::Ratio, "Ratio"),
        (K::Knee, "Knee"),
    ];
    for (i, (k, label)) in row1.into_iter().enumerate() {
        knob(
            pl,
            e,
            id(k),
            (78.0 + i as f32 * 96.0, 432.0, 24.0),
            &look,
            opts(k, label),
        );
    }
    let row2 = [
        (K::GainIn, "Gain In"),
        (K::Mix, "Mix"),
        (K::Wet, "Wet"),
        (K::Dry, "Dry"),
    ];
    for (i, (k, label)) in row2.into_iter().enumerate() {
        knob(
            pl,
            e,
            id(k),
            (78.0 + i as f32 * 96.0, 528.0, 20.0),
            &look,
            opts(k, label),
        );
    }
    knob(
        pl,
        e,
        id(K::GainOut),
        (78.0 + 4.0 * 96.0, 528.0, 20.0),
        &look,
        gain_out_opts(e, Mode::VariMu, "Gain Out"),
    );
    // POWER lamp = not bypassed.
    let bypass = e.params().on(Pid::g(G::Bypass));
    lamp(
        pl,
        486.0,
        150.0,
        10.0,
        !bypass,
        Color32::from_rgb(0xFF, 0x3A, 0x2A),
    );
    let r = pl.r(468.0, 168.0, 36.0, 36.0);
    if pl
        .ui
        .interact(r, pl.ui.scope_id().with("vmu-power"), egui::Sense::click())
        .on_hover_text("Power: off = bypass")
        .clicked()
    {
        e.set(Pid::g(G::Bypass), if bypass { 0.0 } else { 1.0 });
    }
    pl.text(486.0, 172.0, "POWER", 10.0, ink);
    let plaque = pl.r(110.0, 592.0, 320.0, 26.0);
    pl.painter.rect_filled(
        plaque,
        CornerRadius::same(pl.s(4.0) as u8),
        Color32::from_rgb(0x8A, 0x6A, 0x36),
    );
    pl.painter.rect_stroke(
        plaque,
        CornerRadius::same(pl.s(4.0) as u8),
        Stroke::new(1.5, Color32::from_rgb(0x5A, 0x42, 0x20)),
        StrokeKind::Inside,
    );
    pl.painter.text(
        plaque.center(),
        Align2::CENTER_CENTER,
        format!("TUBE VARI-MU  ·  SolPlugs v{VERSION}"),
        pl.font(11.0),
        Color32::from_rgb(0x2A, 0x1C, 0x0C),
    );
}

// ---------------------------------------------------------------- Clean: flat modern digital

fn clean(pl: &mut Plate<'_>, e: &Engine, m: &MeterView) {
    let id = |k| Pid::m(Mode::Clean, k);
    let bg = Color32::from_rgb(0x2B, 0x2E, 0x34);
    let ink = Color32::from_rgb(0xE6, 0xE8, 0xEC);
    let dim = Color32::from_rgb(0x9C, 0xA3, 0xAE);
    let accent = Color32::from_rgb(0x5B, 0xC8, 0xE8);
    pl.painter
        .rect_filled(pl.rect, CornerRadius::same(pl.s(12.0) as u8), bg);
    pl.text_at(
        28.0,
        36.0,
        Align2::LEFT_CENTER,
        "COMPRESSOR",
        pl.font(16.0),
        ink,
    );
    let chip = pl.r(150.0, 24.0, 76.0, 24.0);
    pl.painter.rect_filled(
        chip,
        CornerRadius::same(pl.s(12.0) as u8),
        Color32::from_rgb(0x46, 0x52, 0x66),
    );
    pl.painter.text(
        chip.center(),
        Align2::CENTER_CENTER,
        "DIGITAL",
        pl.font(12.0),
        ink,
    );
    // GR readout with bar.
    let gbox = pl.r(28.0, 64.0, 230.0, 74.0);
    pl.painter.rect_filled(
        gbox,
        CornerRadius::same(pl.s(6.0) as u8),
        Color32::from_rgb(0x15, 0x17, 0x1B),
    );
    pl.text_at(
        42.0,
        82.0,
        Align2::LEFT_CENTER,
        "GAIN REDUCTION",
        pl.font(12.0),
        dim,
    );
    pl.text_at(
        42.0,
        106.0,
        Align2::LEFT_CENTER,
        &format!("-{:.1} dB", m.gr),
        pl.mono(20.0),
        ink,
    );
    let track = pl.r(42.0, 124.0, 200.0, 6.0);
    pl.painter.rect_filled(
        track,
        CornerRadius::same(3),
        Color32::from_rgb(0x30, 0x34, 0x3B),
    );
    let mut fill = track;
    fill.set_width(track.width() * (m.gr / 24.0).clamp(0.0, 1.0));
    pl.painter.rect_filled(fill, CornerRadius::same(3), accent);
    let la = e.latency_ms(Mode::Clean);
    pl.text_at(
        512.0,
        82.0,
        Align2::RIGHT_CENTER,
        "LATENCY",
        pl.font(11.0),
        dim,
    );
    pl.text_at(
        512.0,
        104.0,
        Align2::RIGHT_CENTER,
        &format!("{la:.2} ms"),
        pl.mono(15.0),
        ink,
    );
    // Sliders in two columns.
    let items = [
        (K::Threshold, "Threshold"),
        (K::Ratio, "Ratio"),
        (K::Attack, "Attack"),
        (K::Release, "Release"),
        (K::Knee, "Knee"),
        (K::GainOut, "Makeup"),
        (K::GainIn, "Gain In"),
        (K::Lookahead, "Lookahead"),
        (K::Wet, "Wet"),
        (K::Dry, "Dry"),
        (K::Mix, "Mix"),
    ];
    for (i, (k, label)) in items.into_iter().enumerate() {
        let x = if i % 2 == 0 { 28.0 } else { 284.0 };
        let y = 166.0 + (i / 2) as f32 * 76.0;
        slider(pl, e, id(k), (x, y, 228.0), label, ink, dim, accent);
    }
}

/// A flat horizontal slider for the Clean face.
#[allow(clippy::too_many_arguments)]
fn slider(
    pl: &mut Plate<'_>,
    e: &Engine,
    id: Pid,
    (x, y, w): (f32, f32, f32),
    label: &str,
    ink: Color32,
    dim: Color32,
    accent: Color32,
) {
    let Some(info) = id.info() else {
        return;
    };
    let auto =
        id == Pid::m(Mode::Clean, K::GainOut) && e.params().m(Mode::Clean, K::AutoMakeup) >= 0.5;
    let value = if auto {
        f64::from(e.meters().auto_makeup())
    } else {
        e.params().get(id)
    };
    let norm = info.normalize(value) as f32;
    pl.text_at(x, y, Align2::LEFT_CENTER, label, pl.font(14.0), ink);
    let track = pl.r(x, y + 20.0, w, 6.0);
    let hit = track.expand2(vec2(pl.s(8.0), pl.s(12.0)));
    let resp = pl
        .ui
        .interact(
            hit,
            pl.ui.scope_id().with(("slider", id.0)),
            egui::Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if resp.clicked()
        && !auto
        && let Some(p) = resp.interact_pointer_pos()
    {
        let n = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        e.set(id, info.denormalize(f64::from(n)));
    }
    // Horizontal drags move by the pointer's travel along the track.
    if resp.dragged() && !auto {
        let fine = pl.ui.input(|i| i.modifiers.shift);
        let dx = resp.drag_delta().x / track.width() * if fine { 0.2 } else { 1.0 };
        if resp.drag_started() {
            e.begin_edit(id);
        }
        e.edit(
            id,
            info.denormalize(info.normalize(e.params().get(id)) + f64::from(dx)),
        );
    } else {
        plate::drag_logic(pl.ui, e, id, &resp, f64::from(norm), auto);
    }
    if resp.drag_stopped() && !auto {
        e.end_edit(id);
    }
    let p = &pl.painter;
    p.rect_filled(
        track,
        CornerRadius::same(3),
        Color32::from_rgb(0x3A, 0x3F, 0x48),
    );
    let mut fill = track;
    fill.set_width(track.width() * norm);
    p.rect_filled(fill, CornerRadius::same(3), accent.gamma_multiply(0.75));
    let thumb = Rect::from_center_size(
        pos2(track.left() + track.width() * norm, track.center().y),
        vec2(pl.s(22.0), pl.s(18.0)),
    );
    p.rect_filled(
        thumb,
        CornerRadius::same(pl.s(5.0) as u8),
        if resp.hovered() || resp.dragged() {
            Color32::from_rgb(0xB8, 0xC4, 0xD8)
        } else {
            Color32::from_rgb(0x8C, 0x98, 0xAC)
        },
    );
    p.line_segment(
        [
            thumb.center() - vec2(0.0, pl.s(5.0)),
            thumb.center() + vec2(0.0, pl.s(5.0)),
        ],
        Stroke::new(1.0, Color32::from_rgb(0x50, 0x58, 0x66)),
    );
    let text = if auto {
        format!("{:+.1} dB (auto)", value)
    } else {
        crate::params::format(id, value)
    };
    pl.text_at(x, y + 44.0, Align2::LEFT_CENTER, &text, pl.font(12.5), dim);
    let help = format!(
        "{}: {}\n{}",
        info.name,
        text,
        help(match id.decode() {
            Some(crate::params::Which::M(_, k)) => k,
            _ => K::Threshold,
        })
    );
    let _ = resp.on_hover_text(help);
}

/// Needle positions to show for a GR value (no smoothing).
pub fn needle_for(gr: f32) -> f32 {
    gr_to_needle(gr)
}

/// Unused-design guard: keeps the canvas constants referenced from one place.
pub const CANVAS: (f32, f32) = (W, H);
