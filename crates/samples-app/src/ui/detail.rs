//! The detail panel: big waveform with time selection, live spectrum, the sample tools,
//! processing, metadata (BPM, key, genres, tags, rating), versions and packs.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use solplugs_samples_core::edits::{Edit, Render};
use solplugs_samples_core::keys::Key;
use solplugs_samples_core::model::{Sample, Source, Version};
use solplugs_samples_core::peaks;
use solplugs_samples_core::spectrum::{BANDS, FLOOR_DB, Spectrum};
use solplugs_samples_core::tools::{Range, Tool};

use super::theme::{Palette, mix, rgb, section};
use super::widgets::{art, chip, genre_chip, heading, length, stars, waveform};
use super::{Action, UiLocal};
use crate::engine::{Engine, Shared};
use crate::worker::Cmd;

fn source_label(s: Source) -> &'static str {
    match s {
        Source::None => "",
        Source::Tag => "from tag",
        Source::Name => "from name",
        Source::Detected => "Song Sense",
        Source::User => "set by you",
    }
}

pub fn show(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    actions: &mut Vec<Action>,
) {
    let Some(id) = local.selected.clone() else {
        return;
    };
    let Some(smp) = s.lib.sample(&id).cloned() else {
        return;
    };
    egui::ScrollArea::vertical()
        .id_salt("smp-detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            header(ui, s, local, pal, &smp, actions);
            if local.confirm_remove.as_deref() == Some(id.as_str()) {
                confirm_remove(ui, s, local, pal, engine, &smp);
            }
            ui.add_space(8.0);
            let audio = engine.audio(&smp.id, &smp.path);
            wave(ui, s, local, pal, engine, &smp, audio.as_deref());
            if s.lib.settings.spectrum {
                spectrum(ui, local, pal, engine, &smp);
            }
            transport(ui, s, local, pal, engine, &smp);
            ui.add_space(8.0);
            tools(ui, local, pal, engine, &smp, audio.as_deref());
            ui.add_space(8.0);
            fx(ui, s, pal, engine, &smp);
            ui.add_space(8.0);
            metadata(ui, s, local, pal, engine, &smp);
            ui.add_space(8.0);
            versions(ui, s, local, pal, &smp);
            ui.add_space(8.0);
            packs(ui, s, pal, &smp);
            ui.add_space(20.0);
        });
}

fn header(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    smp: &Sample,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::click());
        let colors: Vec<Color32> = smp
            .genres
            .iter()
            .map(|g| rgb(s.lib.genre_color(g)))
            .collect();
        let tex = smp
            .art
            .as_deref()
            .and_then(|a| local.textures.get(ui.ctx(), a));
        art(
            ui.painter(),
            rect,
            tex.as_ref(),
            &colors,
            &smp.name,
            pal,
            CornerRadius::same(10),
        );
        if resp
            .on_hover_text("Choose artwork for this sample")
            .clicked()
        {
            actions.push(Action::PickArt(smp.id.clone()));
        }
        ui.vertical(|ui| {
            let mut name = smp.name.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut name)
                        .font(FontId::proportional(17.0))
                        .desired_width(ui.available_width()),
                )
                .changed()
            {
                if let Some(x) = s.lib.sample_mut(&smp.id) {
                    x.name = name;
                }
                s.touch();
            }
            let info = format!(
                "{} · {} Hz{} · {} · {}",
                smp.file_type.label(),
                smp.sample_rate,
                smp.bit_depth
                    .map_or_else(String::new, |b| format!(" · {b}-bit")),
                if smp.channels == 1 { "mono" } else { "stereo" },
                length(smp.duration_sec)
            );
            ui.label(RichText::new(info).size(12.0).color(pal.text_dim));
            let path = smp.path.to_string_lossy().to_string();
            if ui
                .add(
                    egui::Label::new(
                        RichText::new(super::widgets::ellipsize(&path, 60))
                            .size(11.0)
                            .color(pal.text_faint),
                    )
                    .sense(Sense::click()),
                )
                .on_hover_text(format!("{path}\nClick to show in folder"))
                .clicked()
            {
                super::grid::reveal(&smp.path);
            }
            if let Version::Edited { parent, edit, .. } = &smp.version {
                let from = s
                    .lib
                    .sample(parent)
                    .map_or("a removed sample".to_string(), |p| p.name.clone());
                ui.label(
                    RichText::new(format!("Edited from {from}: {edit}"))
                        .size(11.5)
                        .color(pal.accent),
                );
            }
        });
    });
}

fn confirm_remove(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    smp: &Sample,
) {
    section(pal).show(ui, |ui| {
        let edits = s.lib.children(&smp.id).count();
        let msg = if smp.version.is_edited() {
            "Remove this edit? Its file in the library's edits folder is deleted.".to_string()
        } else {
            format!(
                "Remove from the library? The file stays where it is.{}",
                if edits > 0 {
                    format!(" Its {edits} edit(s) are kept.")
                } else {
                    String::new()
                }
            )
        };
        ui.label(RichText::new(msg).color(pal.text));
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Remove").color(pal.danger))
                .clicked()
            {
                engine.stop();
                engine.forget(&smp.id);
                if let Some(gone) = s.lib.remove_sample(&smp.id)
                    && gone.version.is_edited()
                    && engine.store().owns(&gone.path)
                {
                    let _ = std::fs::remove_file(&gone.path);
                }
                s.touch();
                local.confirm_remove = None;
                local.select(None);
            }
            if ui.button("Keep").clicked() {
                local.confirm_remove = None;
            }
        });
    });
}

fn time_at(rect: Rect, x: f32, dur: f64) -> f64 {
    (f64::from(((x - rect.left()) / rect.width()).clamp(0.0, 1.0)) * dur).clamp(0.0, dur)
}

fn wave(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    smp: &Sample,
    audio: Option<&solplugs_samples_core::audio::Audio>,
) {
    let main = smp
        .genres
        .first()
        .map_or(pal.accent, |g| rgb(s.lib.genre_color(g)));
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 132.0), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(10), pal.bg);
    let inner = rect.shrink2(vec2(8.0, 10.0));
    let dur = smp.duration_sec.max(1e-6);
    // Selection shading under the waveform.
    if let Some((a, b)) = local.selection {
        let x0 = inner.left() + inner.width() * (a / dur) as f32;
        let x1 = inner.left() + inner.width() * (b / dur) as f32;
        p.rect_filled(
            Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom())),
            0.0,
            pal.selection,
        );
        p.line_segment(
            [pos2(x0, rect.top()), pos2(x0, rect.bottom())],
            Stroke::new(1.0, pal.accent),
        );
        p.line_segment(
            [pos2(x1, rect.top()), pos2(x1, rect.bottom())],
            Stroke::new(1.0, pal.accent),
        );
    }
    let player = engine.player();
    let here = player.clip_id().as_deref() == Some(smp.id.as_str());
    let pos = here.then(|| player.position_sec());
    let played = pos
        .filter(|_| player.is_playing())
        .map(|t| (t / dur) as f32);
    let cols = (inner.width() / 2.0).max(1.0) as usize;
    match audio {
        Some(a) => {
            let fresh = local
                .detail_cols
                .as_ref()
                .is_none_or(|(i, n, _, _)| *i != smp.id || *n != cols);
            if fresh {
                let (mn, mx) = peaks::columns(a, cols);
                local.detail_cols = Some((smp.id.clone(), cols, mn, mx));
            }
            if let Some((_, _, mn, mx)) = &local.detail_cols {
                waveform(
                    p,
                    inner,
                    cols,
                    |i| (mn[i], mx[i]),
                    mix(pal.wave, pal.bg, 0.35),
                    mix(main, Color32::WHITE, 0.2),
                    played,
                );
            }
        }
        None => {
            let n = smp.peaks.len();
            waveform(
                p,
                inner,
                n,
                |i| smp.peaks.get(i),
                mix(pal.wave, pal.bg, 0.6),
                main,
                played,
            );
            ui.ctx().request_repaint();
        }
    }
    p.line_segment(
        [
            pos2(inner.left(), inner.center().y),
            pos2(inner.right(), inner.center().y),
        ],
        Stroke::new(1.0, pal.line),
    );
    if let Some(t) = pos.filter(|_| player.is_playing()) {
        let x = inner.left() + inner.width() * (t / dur).clamp(0.0, 1.0) as f32;
        p.line_segment(
            [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)],
            Stroke::new(2.0, Color32::WHITE),
        );
    }
    let label = match local.selection {
        Some((a, b)) => format!("Selection {:.2}–{:.2} s ({:.2} s)", a, b, b - a),
        None => "Drag to select a range · click to play from there".to_string(),
    };
    p.text(
        rect.left_top() + vec2(10.0, 6.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(11.0),
        pal.text_dim,
    );
    p.text(
        rect.right_bottom() - vec2(10.0, 6.0),
        Align2::RIGHT_BOTTOM,
        length(smp.duration_sec),
        FontId::proportional(11.0),
        pal.text_faint,
    );
    // Interaction.
    if resp.drag_started()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        local.sel_anchor = Some(time_at(inner, pt.x, dur));
    }
    if resp.dragged()
        && let (Some(a), Some(pt)) = (local.sel_anchor, resp.interact_pointer_pos())
    {
        let b = time_at(inner, pt.x, dur);
        local.selection = (b - a).abs().gt(&0.005).then(|| (a.min(b), a.max(b)));
    }
    if resp.drag_stopped() {
        local.sel_anchor = None;
        if let Some((a, b)) = local.selection {
            engine.preview(&smp.id, &smp.path, &smp.fx, a, Some(b));
        }
    }
    if resp.clicked()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let t = time_at(inner, pt.x, dur);
        match local.selection {
            Some((a, b)) if t >= a && t <= b => {
                engine.preview(&smp.id, &smp.path, &smp.fx, t, Some(b))
            }
            _ => {
                local.selection = None;
                engine.preview(&smp.id, &smp.path, &smp.fx, t, None);
            }
        }
    }
    let _ = s;
}

fn spectrum(ui: &mut Ui, local: &mut UiLocal, pal: &Palette, engine: &Engine, smp: &Sample) {
    let player = engine.player();
    let clip = player
        .clip()
        .filter(|c| c.id == smp.id && player.is_playing());
    if local.spectrum_for.as_deref() != Some(smp.id.as_str()) {
        local.spectrum = Spectrum::new();
        local.spectrum_for = Some(smp.id.clone());
    }
    match clip {
        Some(c) => {
            let frame = (player.position_sec() * c.audio.sample_rate) as usize;
            local.spectrum.update(&c.audio, frame);
        }
        None => local.spectrum.decay(),
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(10), pal.bg);
    let inner = rect.shrink2(vec2(8.0, 8.0));
    let bw = inner.width() / BANDS as f32;
    for (i, db) in local.spectrum.bands.iter().enumerate() {
        let level = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
        let h = inner.height() * level;
        let x = inner.left() + i as f32 * bw;
        let t = i as f32 / BANDS as f32;
        let c = mix(pal.accent2, pal.accent, t);
        p.rect_filled(
            Rect::from_min_max(
                pos2(x + 0.5, inner.bottom() - h),
                pos2(x + bw - 0.5, inner.bottom()),
            ),
            CornerRadius::same(1),
            c.gamma_multiply(0.35 + 0.65 * level),
        );
    }
    for (hz, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        let t = ((hz / 20.0f32).log10() / 3.0).clamp(0.0, 1.0);
        let x = inner.left() + inner.width() * t;
        p.line_segment(
            [pos2(x, inner.top()), pos2(x, inner.bottom())],
            Stroke::new(1.0, pal.line.gamma_multiply(0.6)),
        );
        p.text(
            pos2(x + 3.0, inner.top()),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(10.0),
            pal.text_faint,
        );
    }
    if player.is_playing() {
        ui.ctx().request_repaint();
    }
}

fn transport(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    smp: &Sample,
) {
    let player = engine.player();
    ui.horizontal(|ui| {
        let playing = player.is_playing() && player.clip_id().as_deref() == Some(smp.id.as_str());
        if ui.button(if playing { "Stop" } else { "Play" }).clicked() {
            if playing {
                engine.stop();
            } else {
                let (from, to) = local.selection.map_or((0.0, None), |(a, b)| (a, Some(b)));
                engine.preview(&smp.id, &smp.path, &smp.fx, from, to);
            }
        }
        let mut lp = player.looping();
        if ui.toggle_value(&mut lp, "Loop").changed() {
            player.set_looping(lp);
        }
        let mut sp = s.lib.settings.spectrum;
        if ui.toggle_value(&mut sp, "Spectrum").changed() {
            s.lib.settings.spectrum = sp;
            s.touch();
        }
        let mut g = s.lib.settings.preview_gain_db;
        ui.spacing_mut().slider_width = 90.0;
        if ui
            .add(
                egui::Slider::new(&mut g, -30.0..=6.0)
                    .fixed_decimals(1)
                    .suffix(" dB"),
            )
            .on_hover_text("Preview level")
            .changed()
        {
            s.lib.settings.preview_gain_db = g;
            player.set_gain_db(g);
            s.touch();
        }
    });
    let _ = pal;
}

fn tools(
    ui: &mut Ui,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    smp: &Sample,
    audio: Option<&solplugs_samples_core::audio::Audio>,
) {
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(
            ui,
            "TOOLS · each saves an edited version linked to this sample",
            pal,
        );
        let Some(a) = audio else {
            ui.label(RichText::new("Loading audio…").color(pal.text_dim));
            return;
        };
        let range = local
            .selection
            .map_or_else(|| Range::all(a), |(x, y)| Range::seconds(a, x, y));
        let scope = if local.selection.is_some() {
            "selection"
        } else {
            "whole sample"
        };
        let fade = ((f64::from(local.fade_ms) / 1000.0) * a.sample_rate) as usize;
        let run = |tool: Tool| {
            engine.send(Cmd::Edit {
                id: smp.id.clone(),
                edit: Edit::Tool(tool),
            })
        };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    local.selection.is_some(),
                    egui::Button::new("Trim to selection"),
                )
                .clicked()
            {
                run(Tool::Trim(range));
            }
            if ui
                .button("Trim silence")
                .on_hover_text("Cut silence off the start and end")
                .clicked()
            {
                run(Tool::TrimSilence);
            }
            if ui
                .button("Reverse")
                .on_hover_text(format!("Reverse the {scope}"))
                .clicked()
            {
                run(Tool::Reverse(range));
            }
            if ui
                .button("Normalize")
                .on_hover_text(format!("Peak of the {scope} to -1 dBFS"))
                .clicked()
            {
                run(Tool::Normalize(range));
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button("Fade in")
                .on_hover_text(format!("Fade in at the start of the {scope}"))
                .clicked()
            {
                run(Tool::FadeIn { range, len: fade });
            }
            if ui
                .button("Fade out")
                .on_hover_text(format!("Fade out at the end of the {scope}"))
                .clicked()
            {
                run(Tool::FadeOut { range, len: fade });
            }
            ui.add(
                egui::Slider::new(&mut local.fade_ms, 1.0..=2000.0)
                    .logarithmic(true)
                    .fixed_decimals(0)
                    .suffix(" ms"),
            );
        });
        ui.add_space(4.0);
        heading(ui, "SAVE SELECTION AS A NEW SAMPLE", pal);
        ui.horizontal(|ui| {
            let has = local.selection.is_some();
            let save = |render: Render| {
                engine.send(Cmd::Edit {
                    id: smp.id.clone(),
                    edit: Edit::Selection { range, render },
                });
            };
            if ui
                .add_enabled(has, egui::Button::new("Pre FX"))
                .on_hover_text("The dry audio of the range")
                .clicked()
            {
                save(Render::PreFx);
            }
            if ui
                .add_enabled(has, egui::Button::new("Post FX"))
                .on_hover_text("The range through this sample's processing")
                .clicked()
            {
                save(Render::PostFx);
            }
            if ui
                .add_enabled(has, egui::Button::new("Both"))
                .on_hover_text("Two files: dry and processed, both linked")
                .clicked()
            {
                save(Render::Both);
            }
            if !has {
                ui.label(
                    RichText::new("Drag on the waveform first")
                        .size(11.0)
                        .color(pal.text_faint),
                );
            }
        });
    });
}

fn fx(ui: &mut Ui, s: &mut Shared, pal: &Palette, engine: &Engine, smp: &Sample) {
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(
            ui,
            "PROCESSING · heard in previews, rendered by Post FX",
            pal,
        );
        let mut f = smp.fx.clone();
        let mut changed = false;
        egui::Grid::new("smp-fx")
            .num_columns(2)
            .spacing(vec2(10.0, 6.0))
            .show(ui, |ui| {
                ui.label("Gain");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut f.gain_db, -24.0..=12.0)
                            .fixed_decimals(1)
                            .suffix(" dB"),
                    )
                    .changed();
                ui.end_row();
                ui.label("Low cut");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut f.low_cut_hz, 0.0..=2000.0)
                            .logarithmic(true)
                            .custom_formatter(|v, _| {
                                if v < 1.0 {
                                    "off".into()
                                } else {
                                    format!("{v:.0} Hz")
                                }
                            }),
                    )
                    .changed();
                ui.end_row();
                ui.label("High cut");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut f.high_cut_hz, 0.0..=20_000.0)
                            .logarithmic(true)
                            .custom_formatter(|v, _| {
                                if v < 1.0 {
                                    "off".into()
                                } else {
                                    format!("{v:.0} Hz")
                                }
                            }),
                    )
                    .changed();
                ui.end_row();
                ui.label("Fade in");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut f.fade_in_ms, 0.0..=2000.0)
                            .fixed_decimals(0)
                            .suffix(" ms"),
                    )
                    .changed();
                ui.end_row();
                ui.label("Fade out");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut f.fade_out_ms, 0.0..=2000.0)
                            .fixed_decimals(0)
                            .suffix(" ms"),
                    )
                    .changed();
                ui.end_row();
                ui.label("");
                changed |= ui
                    .checkbox(&mut f.normalize, "Normalize to -1 dBFS")
                    .changed();
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if !f.is_neutral() && ui.button("Reset").clicked() {
                f = Default::default();
                changed = true;
            }
        });
        if changed {
            if f.low_cut_hz < 1.0 {
                f.low_cut_hz = 0.0;
            }
            if f.high_cut_hz < 1.0 {
                f.high_cut_hz = 0.0;
            }
            engine.refresh_preview(&smp.id, &smp.path, &f);
            if let Some(x) = s.lib.sample_mut(&smp.id) {
                x.fx = f;
            }
            s.touch();
        }
    });
}

fn metadata(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    smp: &Sample,
) {
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, "DETAILS", pal);
        egui::Grid::new("smp-meta")
            .num_columns(2)
            .spacing(vec2(10.0, 8.0))
            .show(ui, |ui| {
                ui.label("BPM");
                ui.horizontal(|ui| {
                    let mut b = smp.bpm.unwrap_or(0.0);
                    let r = ui.add(
                        egui::DragValue::new(&mut b)
                            .range(0.0..=400.0)
                            .speed(0.5)
                            .max_decimals(1)
                            .custom_formatter(|v, _| {
                                if v <= 0.0 {
                                    "–".into()
                                } else {
                                    format!("{v:.1}")
                                }
                            }),
                    );
                    if r.changed()
                        && let Some(x) = s.lib.sample_mut(&smp.id)
                    {
                        x.bpm = (b > 0.0).then_some(b);
                        x.bpm_source = Source::User;
                        s.touch();
                    }
                    for (label, f) in [("½", 0.5), ("×2", 2.0)] {
                        if smp.bpm.is_some() && ui.small_button(label).clicked() {
                            if let Some(x) = s.lib.sample_mut(&smp.id) {
                                x.bpm = x.bpm.map(|v| v * f);
                                x.bpm_source = Source::User;
                            }
                            s.touch();
                        }
                    }
                    ui.label(
                        RichText::new(source_label(smp.bpm_source))
                            .size(11.0)
                            .color(pal.text_faint),
                    );
                });
                ui.end_row();
                ui.label("Key");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("smp-key-edit")
                        .selected_text(smp.key.clone().unwrap_or_else(|| "–".into()))
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            let mut set = |k: Option<String>| {
                                if let Some(x) = s.lib.sample_mut(&smp.id) {
                                    x.key = k;
                                    x.key_source = Source::User;
                                }
                                s.touch();
                            };
                            if ui.selectable_label(smp.key.is_none(), "–").clicked() {
                                set(None);
                            }
                            for k in Key::all() {
                                let n = k.name();
                                if ui
                                    .selectable_label(smp.key.as_deref() == Some(n.as_str()), &n)
                                    .clicked()
                                {
                                    set(Some(n));
                                }
                            }
                        });
                    ui.label(
                        RichText::new(source_label(smp.key_source))
                            .size(11.0)
                            .color(pal.text_faint),
                    );
                    if solplugs_songsense::available()
                        && ui
                            .small_button("Detect")
                            .on_hover_text("Run Song Sense on this sample")
                            .clicked()
                    {
                        engine.send(Cmd::Detect { id: smp.id.clone() });
                    }
                });
                ui.end_row();
                ui.label("Rating");
                ui.horizontal(|ui| {
                    if let Some(r) = stars(ui, smp.rating, pal, 18.0) {
                        if let Some(x) = s.lib.sample_mut(&smp.id) {
                            x.rating = r;
                        }
                        s.touch();
                    }
                    let mut fav = smp.favorite;
                    let label = "Favorite";
                    if ui.toggle_value(&mut fav, label).changed() {
                        if let Some(x) = s.lib.sample_mut(&smp.id) {
                            x.favorite = fav;
                        }
                        s.touch();
                    }
                });
                ui.end_row();
                ui.label("Genres");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
                    for g in &smp.genres {
                        if genre_chip(ui, &format!("{g}  ×"), rgb(s.lib.genre_color(g)), true)
                            .on_hover_text("Remove")
                            .clicked()
                        {
                            if let Some(x) = s.lib.sample_mut(&smp.id) {
                                x.genres.retain(|y| y != g);
                            }
                            s.touch();
                        }
                    }
                    egui::ComboBox::from_id_salt("smp-add-genre")
                        .selected_text("+ genre")
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            let names: Vec<String> =
                                s.lib.genres.iter().map(|g| g.name.clone()).collect();
                            for n in names {
                                if !smp.genres.contains(&n)
                                    && ui.selectable_label(false, &n).clicked()
                                {
                                    if let Some(x) = s.lib.sample_mut(&smp.id) {
                                        x.genres.push(n);
                                    }
                                    s.touch();
                                }
                            }
                            ui.separator();
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut local.new_genre)
                                    .hint_text("New genre")
                                    .desired_width(110.0),
                            );
                            if r.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && !local.new_genre.trim().is_empty()
                            {
                                let g = local.new_genre.trim().to_string();
                                if !s.lib.genres.iter().any(|x| x.name.eq_ignore_ascii_case(&g)) {
                                    let color = solplugs_samples_core::genres::fallback_color(&g);
                                    s.lib.genres.push(solplugs_samples_core::model::Genre {
                                        name: g.clone(),
                                        color,
                                    });
                                }
                                if let Some(x) = s.lib.sample_mut(&smp.id) {
                                    x.genres.push(g);
                                }
                                local.new_genre.clear();
                                s.touch();
                            }
                        });
                });
                ui.end_row();
                ui.label("Tags");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
                    for t in &smp.tags {
                        if chip(
                            ui,
                            &format!("#{t}  ×"),
                            pal.accent2.gamma_multiply(0.25),
                            pal.text,
                        )
                        .on_hover_text("Remove")
                        .clicked()
                        {
                            if let Some(x) = s.lib.sample_mut(&smp.id) {
                                x.tags.retain(|y| y != t);
                            }
                            s.touch();
                        }
                    }
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut local.new_tag)
                            .hint_text("add tag")
                            .desired_width(90.0),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let t = local.new_tag.trim().trim_start_matches('#').to_lowercase();
                        if !t.is_empty() && !smp.tags.contains(&t) {
                            if let Some(x) = s.lib.sample_mut(&smp.id) {
                                x.tags.push(t);
                            }
                            s.touch();
                        }
                        local.new_tag.clear();
                        r.request_focus();
                    }
                });
                ui.end_row();
                if !smp.artist.is_empty() {
                    ui.label("Artist");
                    ui.label(RichText::new(&smp.artist).color(pal.text_dim));
                    ui.end_row();
                }
                if !smp.comment.is_empty() {
                    ui.label("Notes");
                    ui.label(RichText::new(&smp.comment).color(pal.text_dim));
                    ui.end_row();
                }
                ui.label("Added");
                ui.label(RichText::new(date(smp.added_unix)).color(pal.text_dim));
                ui.end_row();
            });
    });
}

/// "2026-11-14" from Unix seconds (UTC), without a date library.
fn date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

fn versions(ui: &mut Ui, s: &mut Shared, local: &mut UiLocal, pal: &Palette, smp: &Sample) {
    let root = s.lib.root_of(&smp.id).to_string();
    let mut family: Vec<(usize, Sample)> = Vec::new();
    fn walk(
        lib: &solplugs_samples_core::model::Library,
        id: &str,
        depth: usize,
        out: &mut Vec<(usize, Sample)>,
    ) {
        if depth > 12 {
            return;
        }
        if let Some(x) = lib.sample(id) {
            out.push((depth, x.clone()));
        }
        let kids: Vec<String> = lib.children(id).map(|c| c.id.clone()).collect();
        for k in kids {
            walk(lib, &k, depth + 1, out);
        }
    }
    walk(&s.lib, &root, 0, &mut family);
    if s.lib.sample(&root).is_none() {
        // The raw was removed; show this sample's own branch.
        walk(&s.lib, &smp.id, 0, &mut family);
    }
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, &format!("VERSIONS · {}", family.len()), pal);
        for (depth, v) in family {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                let tag = if v.version.is_edited() { "EDIT" } else { "RAW" };
                let color = if v.version.is_edited() {
                    pal.accent
                } else {
                    pal.accent2
                };
                ui.label(RichText::new(tag).size(10.0).strong().color(color));
                let on = v.id == smp.id;
                let text = match &v.version {
                    Version::Edited { edit, .. } => format!("{} — {edit}", v.name),
                    Version::Raw => v.name.clone(),
                };
                if ui
                    .selectable_label(on, super::widgets::ellipsize(&text, 54))
                    .clicked()
                {
                    local.select(Some(v.id.clone()));
                }
            });
        }
    });
}

fn packs(ui: &mut Ui, s: &mut Shared, pal: &Palette, smp: &Sample) {
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, "PACKS", pal);
        ui.horizontal_wrapped(|ui| {
            let mine: Vec<(String, String)> = s
                .lib
                .packs
                .iter()
                .filter(|p| p.members.contains(&smp.id))
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect();
            for (pid, name) in &mine {
                if chip(
                    ui,
                    &format!("{name}  ×"),
                    pal.accent.gamma_multiply(0.3),
                    pal.text,
                )
                .on_hover_text("Remove from this pack")
                .clicked()
                {
                    if let Some(p) = s.lib.pack_mut(pid) {
                        p.members.retain(|m| *m != smp.id);
                    }
                    s.touch();
                }
            }
            let others: Vec<(String, String)> = s
                .lib
                .packs
                .iter()
                .filter(|p| !p.members.contains(&smp.id))
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect();
            if others.is_empty() && mine.is_empty() {
                ui.label(
                    RichText::new("Create a pack in the sidebar, then add samples here.")
                        .size(11.0)
                        .color(pal.text_faint),
                );
            }
            if !others.is_empty() {
                egui::ComboBox::from_id_salt("smp-add-pack")
                    .selected_text("+ Add to pack")
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (pid, name) in others {
                            if ui.selectable_label(false, name).clicked() {
                                if let Some(p) = s.lib.pack_mut(&pid) {
                                    p.members.push(smp.id.clone());
                                }
                                s.touch();
                            }
                        }
                    });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(1_794_614_400), "2026-11-14");
    }
}
