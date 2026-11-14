//! Clean's view (the locked v5 look): the big EQ graph on the left (about two thirds of the
//! window) with a piano strip and the selected band's controls under it, and a narrow side
//! column on the right (spectrometer with its view toggle, BEFORE/AFTER waveforms, big
//! BEFORE/AFTER meters with dB readouts, MATCH).
//!
//! On the graph: drag a node to move its band (frequency and gain), scroll on it to change
//! Q, right-click for shape and remove, double-click to zero its gain. Click on empty
//! graph to grab the spectrum: a band appears on the nearest peak, at the gain you clicked.

use egui::{
    Align2, Color32, ComboBox, CornerRadius, CursorIcon, FontId, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use solplugs_eq_core::filter::SLOPES;
use solplugs_eq_core::{BandSpec, CleanBand, MAX_BANDS, Place, Shape, Style};

use super::analyzer::Which;
use super::digital::{Cursor, bar_pos, spectrometer, view_toggle};
use super::graph::{self, Axes, hz_short};
use super::theme::Palette;
use super::{UiLocal, db_text, match_row, waveform};
use crate::engine::Engine;
use crate::params::{self, B, C, Pid};

/// Band colours, cycled.
const BAND_COLOURS: [Color32; 8] = [
    Color32::from_rgb(0x4F, 0xE3, 0xF0),
    Color32::from_rgb(0x7C, 0xA8, 0xFF),
    Color32::from_rgb(0xB4, 0x8C, 0xFF),
    Color32::from_rgb(0xFF, 0x7E, 0xC8),
    Color32::from_rgb(0xFF, 0xA8, 0x4D),
    Color32::from_rgb(0xFF, 0xE0, 0x5A),
    Color32::from_rgb(0x8B, 0xE6, 0x6E),
    Color32::from_rgb(0x4D, 0xD9, 0xAE),
];

/// The main curve colour.
const CURVE: Color32 = Color32::from_rgb(0x4F, 0xE3, 0xF0);

fn band_colour(b: usize) -> Color32 {
    BAND_COLOURS[b % BAND_COLOURS.len()]
}

/// A band as the graph shows it: its spec with the live dynamic gain.
struct Shown {
    index: usize,
    band: CleanBand,
    spec: BandSpec,
}

fn shown_bands(engine: &Engine) -> Vec<Shown> {
    let p = engine.params();
    (0..MAX_BANDS)
        .filter_map(|i| {
            let band = p.band(i);
            band.used.then(|| {
                let mut spec = band.spec();
                if band.dynamic && band.enabled {
                    spec.gain_db += f64::from(engine.meters().dynamic(i));
                }
                Shown {
                    index: i,
                    band,
                    spec,
                }
            })
        })
        .collect()
}

/// The rate the curves are drawn at: what the audio really does.
fn display_rate(engine: &Engine) -> f64 {
    let sr = f64::from(engine.sample_rate());
    if engine.params().c(Style::Clean, C::Phase) >= 0.5 {
        sr * 8.0
    } else {
        sr
    }
}

/// Combined curve of all bands, dB at `f`.
pub fn combined(engine: &Engine) -> impl Fn(f32) -> f32 {
    let specs: Vec<BandSpec> = shown_bands(engine).into_iter().map(|s| s.spec).collect();
    let fs = display_rate(engine);
    move |f| {
        specs
            .iter()
            .map(|s| s.magnitude_db(f64::from(f), fs))
            .sum::<f64>() as f32
    }
}

/// Draws Clean's view into `area`.
pub fn show(ui: &mut Ui, area: Rect, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let col_w = (area.width() * 0.3).clamp(220.0, 360.0);
    let left = Rect::from_min_max(area.min, pos2(area.right() - col_w - 10.0, area.bottom()));
    let right = Rect::from_min_max(pos2(left.right() + 10.0, area.top()), area.max);
    let compact = area.height() < 520.0;
    let panel_h = if compact { 96.0 } else { 124.0 };
    let piano_h = if compact { 22.0 } else { 34.0 };
    let graph_r = Rect::from_min_max(left.min, pos2(left.right(), left.bottom() - panel_h - piano_h - 18.0));
    let painter = ui.painter_at(left.expand(1.0));
    painter.rect_filled(graph_r, CornerRadius::same(10), pal.card);
    let plot = Rect::from_min_max(
        graph_r.min + vec2(46.0, 12.0),
        graph_r.max - vec2(12.0, 20.0),
    );
    let a = Axes {
        rect: plot,
        db_range: local.db_range,
        spec: (-90.0, 0.0),
    };
    graph_view(ui, &painter, &a, engine, local, pal);
    let piano = Rect::from_min_size(
        pos2(plot.left(), graph_r.bottom() + 4.0),
        vec2(plot.width(), piano_h),
    );
    piano_strip(&painter, &a, piano, pal);
    let panel = Rect::from_min_max(pos2(left.left(), left.bottom() - panel_h), left.max);
    band_panel(ui, panel, engine, local, pal);
    side_column(ui, right, engine, local, pal);
}

fn graph_view(ui: &mut Ui, p: &egui::Painter, a: &Axes, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let plot = a.rect;
    graph::grid(p, a, pal, true, true);
    // Spectrum scale on the right.
    let clip = p.with_clip_rect(plot);
    graph::spectrum(&clip, a, &local.analyzer, Which::Before, pal.before.gamma_multiply(0.8), Some(pal.before.gamma_multiply(0.12)));
    graph::spectrum(&clip, a, &local.analyzer, Which::After, CURVE.gamma_multiply(0.55), Some(CURVE.gamma_multiply(0.10)));

    let bands = shown_bands(engine);
    let fs = display_rate(engine);
    // Empty-graph clicks: spectrum grab. (Registered before the nodes, so nodes win.)
    let bg = ui
        .interact(plot, ui.scope_id().with("clean-graph"), Sense::click())
        .on_hover_text("Click a peak on the analyzer to put a band there (at the gain you click). Drag nodes to move bands, scroll on a node for Q, right-click a node for its shape.");
    if bg.clicked()
        && let Some(pos) = bg.interact_pointer_pos()
    {
        let fc = a.f(pos.x);
        let f = local.analyzer.peak_near(Which::Before, fc).unwrap_or(fc);
        let gain = (a.db(pos.y) * 10.0).round() / 10.0;
        if let Some(b) = engine.add_band(Shape::Bell, f64::from(f), f64::from(gain), 2.0) {
            local.selected = Some(b);
        }
    }

    // Individual band curves under the combined one.
    if local.show_bands {
        for s in &bands {
            if !s.spec.on {
                continue;
            }
            let c = band_colour(s.index);
            let sel = local.selected == Some(s.index);
            if sel {
                graph::curve_fill(&clip, a, c.gamma_multiply(0.18), |f| s.spec.magnitude_db(f64::from(f), fs) as f32);
            }
            graph::curve(&clip, a, Stroke::new(if sel { 1.6 } else { 1.0 }, c.gamma_multiply(if sel { 0.9 } else { 0.55 })), |f| {
                s.spec.magnitude_db(f64::from(f), fs) as f32
            });
        }
    }
    let total = {
        let specs: Vec<BandSpec> = bands.iter().map(|s| s.spec).collect();
        move |f: f32| specs.iter().map(|s| s.magnitude_db(f64::from(f), fs)).sum::<f64>() as f32
    };
    graph::curve(&clip, a, Stroke::new(2.6, CURVE), &total);

    // Nodes.
    for s in &bands {
        node(ui, p, a, engine, local, s, &total, pal);
    }
    // Crosshair readout.
    if let Some(h) = bg.hover_pos() {
        let f = a.f(h.x);
        let midi = 69.0 + 12.0 * (f / 440.0).log2();
        let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        let n = midi.round() as i32;
        let note = format!("{}{}", names[n.rem_euclid(12) as usize], n.div_euclid(12) - 1);
        p.text(
            plot.right_top() + vec2(-6.0, 4.0),
            Align2::RIGHT_TOP,
            format!("{}  {note}  {:+.1} dB", params::hz_text(f64::from(f)), a.db(h.y)),
            FontId::monospace(11.0),
            pal.dim,
        );
    }
    if bands.is_empty() {
        p.text(
            plot.center() - vec2(0.0, 20.0),
            Align2::CENTER_CENTER,
            "Click the graph to add a band",
            FontId::proportional(15.0),
            pal.dim,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn node(
    ui: &mut Ui,
    p: &egui::Painter,
    a: &Axes,
    engine: &Engine,
    local: &mut UiLocal,
    s: &Shown,
    total: &dyn Fn(f32) -> f32,
    pal: &Palette,
) {
    let b = s.index;
    let f = s.band.freq;
    let y = if s.band.shape.has_gain() {
        a.y(s.band.gain_db)
    } else {
        a.y(total(f).clamp(-a.db_range, a.db_range))
    };
    let c = pos2(a.x(f), y);
    let rad = 9.0;
    let resp = ui
        .interact(
            Rect::from_center_size(c, vec2(rad * 2.6, rad * 2.6)),
            ui.scope_id().with(("node", b)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::Grab);
    let fid = Pid::b(b, B::Freq);
    let gid = Pid::b(b, B::Gain);
    if resp.drag_started() {
        local.selected = Some(b);
        engine.begin_edit(fid);
        if s.band.shape.has_gain() {
            engine.begin_edit(gid);
        }
    }
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        engine.edit(fid, f64::from(a.f(pos.x)));
        if s.band.shape.has_gain() {
            engine.edit(gid, f64::from((a.db(pos.y) * 10.0).round() / 10.0));
        }
    }
    if resp.drag_stopped() {
        engine.end_edit(fid);
        if s.band.shape.has_gain() {
            engine.end_edit(gid);
        }
    }
    if resp.clicked() {
        local.selected = Some(b);
    }
    if resp.double_clicked() {
        engine.set(gid, 0.0);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.5 {
            let qid = Pid::b(b, B::Q);
            if let Some(info) = qid.info() {
                let n = info.normalize(engine.params().get(qid)) + f64::from(scroll) / 600.0;
                engine.set(qid, info.denormalize(n));
            }
        }
    }
    resp.context_menu(|ui| {
        for shape in Shape::ALL {
            if ui.selectable_label(s.band.shape == shape, shape.name()).clicked() {
                engine.set(Pid::b(b, B::Shape), shape.index() as f64);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Remove band").clicked() {
            engine.remove_band(b);
            if local.selected == Some(b) {
                local.selected = None;
            }
            ui.close();
        }
    });
    let col = band_colour(b);
    let sel = local.selected == Some(b);
    let active = s.band.enabled;
    p.circle_filled(c, rad, if sel { col } else { pal.card.gamma_multiply(0.9) });
    p.circle_stroke(c, rad, Stroke::new(2.0, if active { col } else { pal.dim }));
    p.text(
        c,
        Align2::CENTER_CENTER,
        format!("{}", b + 1),
        FontId::proportional(11.5),
        if sel { pal.bg } else { pal.text },
    );
    if s.band.dynamic && active {
        // Dynamic: a ring showing where the gain is now.
        let dy = a.y(s.spec.gain_db as f32);
        p.line_segment([c, pos2(c.x, dy)], Stroke::new(2.0, col.gamma_multiply(0.6)));
        p.circle_stroke(pos2(c.x, dy), 4.0, Stroke::new(1.5, col));
    }
    if s.band.solo {
        p.circle_stroke(c, rad + 4.0, Stroke::new(1.5, pal.sun));
    }
    // Label: number, shape and gain.
    let label = if s.band.shape.has_gain() {
        format!("B{} {} {:+.1}", b + 1, s.band.shape.name(), s.band.gain_db)
    } else {
        format!("B{} {}", b + 1, s.band.shape.name())
    };
    let above = c.y > a.rect.top() + 30.0;
    p.text(
        c + vec2(0.0, if above { -rad - 3.0 } else { rad + 3.0 }),
        if above { Align2::CENTER_BOTTOM } else { Align2::CENTER_TOP },
        label,
        FontId::proportional(10.5),
        col,
    );
    if resp.hovered() || resp.dragged() {
        p.circle_stroke(c, rad + 2.0, Stroke::new(1.0, Color32::from_white_alpha(120)));
        let _ = resp.on_hover_text(format!(
            "Band {}: {} at {}, {:+.1} dB, Q {:.2}\nDrag to move, scroll for Q, right-click for shape, double-click for 0 dB.",
            b + 1,
            s.band.shape.name(),
            params::hz_text(f64::from(s.band.freq)),
            s.band.gain_db,
            s.band.q
        ));
    }
}

/// The piano strip: one column per semitone on the same log axis, C labels underneath.
fn piano_strip(p: &egui::Painter, a: &Axes, r: Rect, pal: &Palette) {
    p.rect_filled(r, CornerRadius::same(3), Color32::from_gray(235));
    let key_h = r.height() - 12.0;
    let black = [1, 3, 6, 8, 10];
    for n in 16..=135 {
        let f = 440.0 * 2f32.powf((n as f32 - 69.0) / 12.0);
        let x0 = a.x(f * 2f32.powf(-1.0 / 24.0)).max(r.left());
        let x1 = a.x(f * 2f32.powf(1.0 / 24.0)).min(r.right());
        if x1 <= x0 {
            continue;
        }
        let pc = n % 12;
        if black.contains(&pc) {
            p.rect_filled(
                Rect::from_min_max(pos2(x0, r.top()), pos2(x1, r.top() + key_h * 0.62)),
                CornerRadius::ZERO,
                Color32::from_gray(25),
            );
        } else if pc == 4 || pc == 11 || pc == 0 || pc == 5 {
            // Natural-key boundaries (E-F and B-C).
            let x = if pc == 0 || pc == 5 { x0 } else { x1 };
            p.line_segment([pos2(x, r.top()), pos2(x, r.top() + key_h)], Stroke::new(1.0, Color32::from_gray(150)));
        }
        if pc == 0 {
            p.text(
                pos2((x0 + x1) / 2.0, r.bottom() - 1.0),
                Align2::CENTER_BOTTOM,
                format!("C{}", n / 12 - 1),
                FontId::proportional(9.5),
                Color32::from_gray(60),
            );
        }
    }
    p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, pal.grid), StrokeKind::Inside);
}

/// A horizontal slider bound to a parameter, with a big readout above it.
fn big_slider(ui: &mut Ui, r: Rect, engine: &Engine, id: Pid, title: &str, pal: &Palette) {
    let Some(info) = id.info() else {
        return;
    };
    let p = ui.painter_at(r);
    let v = engine.params().get(id);
    let small = r.height() < 60.0;
    p.text(
        pos2(r.center().x, r.top()),
        Align2::CENTER_TOP,
        title,
        FontId::proportional(if small { 11.0 } else { 13.0 }),
        pal.dim,
    );
    p.text(
        pos2(r.center().x, r.top() + if small { 13.0 } else { 17.0 }),
        Align2::CENTER_TOP,
        params::format(id, v),
        FontId::proportional(if small { 15.0 } else { 21.0 }),
        CURVE,
    );
    let ty = r.bottom() - 8.0;
    let track = Rect::from_min_max(pos2(r.left() + 6.0, ty - 2.0), pos2(r.right() - 6.0, ty + 2.0));
    p.rect_filled(track, CornerRadius::same(2), pal.raised);
    let n = info.normalize(v) as f32;
    let hx = track.left() + n * track.width();
    p.rect_filled(
        Rect::from_center_size(pos2(hx, ty), vec2(16.0, 12.0)),
        CornerRadius::same(4),
        CURVE,
    );
    let resp = ui
        .interact(
            Rect::from_min_max(pos2(r.left(), ty - 10.0), pos2(r.right(), ty + 10.0)),
            ui.scope_id().with(("slider", id.0)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    if resp.drag_started() || resp.clicked() {
        engine.begin_edit(id);
    }
    if (resp.dragged() || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let n = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        engine.edit(id, info.denormalize(f64::from(n)));
    }
    if resp.drag_stopped() || resp.clicked() {
        engine.end_edit(id);
    }
    if resp.double_clicked() {
        engine.set(id, info.default);
    }
}

fn band_panel(ui: &mut Ui, r: Rect, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let p = ui.painter_at(r);
    p.rect_filled(r, CornerRadius::same(10), pal.card);
    let bands = shown_bands(engine);
    if local.selected.is_some_and(|s| !engine.params().band(s).used) {
        local.selected = None;
    }
    if local.selected.is_none() {
        local.selected = bands.first().map(|s| s.index);
    }
    let inner = r.shrink2(vec2(12.0, 6.0));
    let compact = r.height() < 110.0;
    let top_h = if compact { 46.0 } else { 62.0 };
    let top = Rect::from_min_size(inner.min, vec2(inner.width(), top_h));
    let rows = Rect::from_min_max(pos2(inner.left(), top.bottom() + 4.0), inner.max);
    // Gain / Frequency / Q for the selected band.
    if let Some(sel) = local.selected {
        let w = top.width() / 3.0;
        for (i, (k, t)) in [(B::Gain, "Gain"), (B::Freq, "Frequency"), (B::Q, "Q")].into_iter().enumerate() {
            let cell = Rect::from_min_size(pos2(top.left() + i as f32 * w, top.top()), vec2(w - 10.0, top_h));
            big_slider(ui, cell, engine, Pid::b(sel, k), t, pal);
        }
    } else {
        p.text(top.center(), Align2::CENTER_CENTER, "No bands yet: click the graph or press + Band", FontId::proportional(13.0), pal.dim);
    }
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 3.0);
            for s in &bands {
                let sel = local.selected == Some(s.index);
                let c = band_colour(s.index);
                let text = RichText::new(format!("B{}", s.index + 1)).strong().color(if sel { pal.bg } else if s.band.enabled { c } else { pal.dim });
                let b = egui::Button::new(text).fill(if sel { c } else { pal.raised }).corner_radius(CornerRadius::same(6));
                if ui.add(b).clicked() {
                    local.selected = Some(s.index);
                }
            }
            if bands.len() < MAX_BANDS
                && ui.button("+ Band").on_hover_text("Add a bell at 1 kHz (or click the graph).").clicked()
                && let Some(b) = engine.add_band(Shape::Bell, 1000.0, 0.0, 1.0)
            {
                local.selected = Some(b);
            }
            ui.separator();
            ui.checkbox(&mut local.show_bands, "Show Bands")
                .on_hover_text("Show each band's own curve under the combined curve.");
            let range = local.db_range;
            if ui
                .button(format!("±{range:.0} dB"))
                .on_hover_text("Graph range: ±12, ±24 or ±30 dB.")
                .clicked()
            {
                local.db_range = match range as i32 {
                    12 => 24.0,
                    24 => 30.0,
                    _ => 12.0,
                };
            }
            if let Some(sel) = local.selected {
                ui.separator();
                band_controls(ui, engine, local, sel, pal);
            }
        });
    });
}

fn toggle(ui: &mut Ui, engine: &Engine, id: Pid, label: &str, colour: Color32, help: &str, pal: &Palette) {
    let on = engine.params().on(id);
    let b = egui::Button::new(RichText::new(label).color(if on { pal.bg } else { pal.text }))
        .fill(if on { colour } else { pal.raised })
        .corner_radius(CornerRadius::same(6));
    if ui.add(b).on_hover_text(help).clicked() {
        engine.set(id, if on { 0.0 } else { 1.0 });
    }
}

fn choice(ui: &mut Ui, engine: &Engine, id: Pid, salt: &str, list: &[&str], width: f32) {
    let v = engine.params().get(id) as usize;
    ComboBox::from_id_salt(salt)
        .width(width)
        .selected_text(list.get(v).copied().unwrap_or(""))
        .show_ui(ui, |ui| {
            for (i, l) in list.iter().enumerate() {
                if ui.selectable_label(i == v, *l).clicked() {
                    engine.set(id, i as f64);
                }
            }
        });
}

fn band_controls(ui: &mut Ui, engine: &Engine, local: &mut UiLocal, b: usize, pal: &Palette) {
    let band = engine.params().band(b);
    let shapes: Vec<&str> = Shape::ALL.iter().map(|s| s.name()).collect();
    choice(ui, engine, Pid::b(b, B::Shape), &format!("shape{b}"), &shapes, 92.0);
    if band.shape.has_slope() {
        let slopes: Vec<String> = SLOPES.iter().map(|s| format!("{s} dB/oct")).collect();
        let refs: Vec<&str> = slopes.iter().map(String::as_str).collect();
        choice(ui, engine, Pid::b(b, B::Slope), &format!("slope{b}"), &refs, 90.0);
    }
    for (i, place) in Place::ALL.iter().enumerate() {
        let sel = band.place == *place;
        let t = RichText::new(place.name()).color(if sel { pal.bg } else { pal.text });
        if ui
            .add(egui::Button::new(t).fill(if sel { pal.before } else { pal.raised }).corner_radius(CornerRadius::same(6)))
            .on_hover_text("Stereo: both channels. Mid: what's in the middle (L+R). Side: the stereo width (L-R).")
            .clicked()
        {
            engine.set(Pid::b(b, B::Place), i as f64);
        }
    }
    toggle(ui, engine, Pid::b(b, B::On), "On", pal.good, "Switch this band in or out.", pal);
    let solo = band.solo;
    let t = RichText::new("Solo").color(if solo { pal.bg } else { pal.text });
    if ui
        .add(egui::Button::new(t).fill(if solo { pal.sun } else { pal.raised }).corner_radius(CornerRadius::same(6)))
        .on_hover_text("Solo: hear only what this band touches.")
        .clicked()
    {
        engine.solo_band(b, !solo);
    }
    toggle(ui, engine, Pid::b(b, B::Dynamic), "Dynamic", pal.after, "Dynamic EQ: the band's gain moves by Range when the level in its own frequency range passes Threshold.", pal);
    if band.dynamic {
        for (k, help) in [
            (B::Threshold, "Level (in the band's range) where the dynamics start."),
            (B::Range, "How far the gain moves past the threshold. Negative cuts, positive boosts."),
            (B::Attack, "How fast the band reacts."),
            (B::Release, "How fast it lets go."),
        ] {
            let id = Pid::b(b, k);
            ui.label(RichText::new(id.info().map_or("", |i| i.short.as_str())).color(pal.dim));
            super::value_box(ui, engine, id, help, pal, None);
        }
    }
    if ui.button("Remove").on_hover_text("Remove this band.").clicked() {
        engine.remove_band(b);
        local.selected = None;
    }
}

fn side_column(ui: &mut Ui, area: Rect, engine: &Engine, local: &mut UiLocal, pal: &Palette) {
    let painter = ui.painter_at(area.expand(1.0));
    painter.rect_filled(area, CornerRadius::same(10), pal.panel);
    let inner = area.shrink(10.0);
    let compact = inner.height() < 560.0;
    let gap = if compact { 5.0 } else { 8.0 };
    let (title, toggle_h, spec_h, wave_h, matchb, foot) = if compact {
        (16.0, 24.0, inner.height() * 0.22, 26.0, 34.0, 14.0)
    } else {
        (20.0, 30.0, inner.height() * 0.24, 40.0, 46.0, 16.0)
    };
    let mut cur = Cursor {
        rect: inner,
        y: inner.top(),
        gap,
    };
    let t = cur.take(title);
    painter.text(t.left_center(), Align2::LEFT_CENTER, "SOURCE / OUTPUT", FontId::proportional(13.5), pal.text);
    let tr = cur.take(toggle_h);
    view_toggle(ui, &painter, tr, &mut local.spec_view, pal);
    let curve = combined(engine);
    spectrometer(&painter, cur.take(spec_h), &local.analyzer, local.spec_view, &curve, pal, false);
    waveform(&painter, cur.take(wave_h), &local.history, false, pal);
    waveform(&painter, cur.take(wave_h), &local.history, true, pal);
    let fixed_after = matchb + foot * 2.0 + gap * 3.0;
    let meters_h = (inner.bottom() - cur.y - fixed_after).max(60.0);
    big_meters(&painter, cur.take(meters_h), engine, pal);
    match_row(ui, &painter, cur.take(matchb), engine, pal);
    let m = engine.meters();
    let diff = m.avg_after() - m.avg_before();
    let d = cur.take(foot);
    painter.text(
        d.center(),
        Align2::CENTER_CENTER,
        if m.avg_before() > -90.0 {
            format!("AFTER − BEFORE: {diff:+.1} dB")
        } else {
            "Play audio to measure".to_owned()
        },
        FontId::proportional(12.0),
        pal.text,
    );
    let f = cur.take(foot);
    let linear = engine.params().c(Style::Clean, C::Phase) >= 0.5;
    painter.text(
        f.center(),
        Align2::CENTER_CENTER,
        format!(
            "Latency {:.1} ms · {:.0} kHz · {}",
            engine.latency_ms(Style::Clean),
            engine.sample_rate() / 1000.0,
            if linear { "Linear phase" } else { "Zero latency" }
        ),
        FontId::proportional(10.5),
        pal.dim,
    );
}

/// Big BEFORE and AFTER meters (average peak, the post meter) with tick scales and small
/// dB readouts.
fn big_meters(p: &egui::Painter, r: Rect, engine: &Engine, pal: &Palette) {
    let m = engine.meters();
    let label_h = 18.0;
    let read_h = 20.0;
    let bar_w = (r.width() * 0.2).clamp(18.0, 56.0);
    for (i, (label, v, c)) in [("BEFORE", m.avg_before(), pal.before), ("AFTER", m.avg_after(), pal.after)]
        .into_iter()
        .enumerate()
    {
        let cx = r.left() + r.width() * (0.27 + 0.46 * i as f32);
        p.text(pos2(cx, r.top()), Align2::CENTER_TOP, label, FontId::proportional(13.0), c);
        let bar = Rect::from_min_max(
            pos2(cx - bar_w / 2.0, r.top() + label_h + 2.0),
            pos2(cx + bar_w / 2.0, r.bottom() - read_h - 2.0),
        );
        // Ticks every 6 dB on both sides.
        for k in 0..=10 {
            let y = bar.bottom() - bar.height() * k as f32 / 10.0;
            for (x0, x1) in [(bar.left() - 9.0, bar.left() - 3.0), (bar.right() + 3.0, bar.right() + 9.0)] {
                p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0, pal.dim.gamma_multiply(0.6)));
            }
        }
        p.rect_filled(bar, CornerRadius::same(4), pal.bg);
        let top = bar.bottom() - bar.height() * bar_pos(v);
        p.rect_filled(Rect::from_min_max(pos2(bar.left(), top), bar.max), CornerRadius::same(4), c);
        p.text(
            pos2(cx, r.bottom()),
            Align2::CENTER_BOTTOM,
            db_text(v),
            FontId::proportional(15.0),
            c,
        );
    }
    p.text(
        pos2(r.center().x, r.top() + label_h + 4.0),
        Align2::CENTER_TOP,
        "0",
        FontId::proportional(9.5),
        pal.dim,
    );
    p.text(
        pos2(r.center().x, r.bottom() - read_h - 4.0),
        Align2::CENTER_BOTTOM,
        "-60",
        FontId::proportional(9.5),
        pal.dim,
    );
    let _ = hz_short;
}
