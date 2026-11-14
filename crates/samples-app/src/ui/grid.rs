//! The card grid: every sample is a big card with artwork, its waveform, name, BPM, key and
//! length, in its genre colours. Click plays, drag drops it into a track or DAW.

use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use solplugs_samples_core::model::{Sample, Version};

use super::theme::{Palette, mix, rgb};
use super::widgets::{art, bpm, ellipsize, fav_button, length, scrim, short_key, star, waveform};
use super::{Action, UiLocal, View};
use crate::dragout::SampleDrag;
use crate::engine::{Engine, Shared};

const GAP: f32 = 14.0;
const TEXT_H: f32 = 64.0;

pub fn show(
    ui: &mut Ui,
    s: &mut Shared,
    local: &mut UiLocal,
    pal: &Palette,
    engine: &Engine,
    actions: &mut Vec<Action>,
) {
    let results = local.results(s);
    if s.lib.samples.is_empty() {
        empty(
            ui,
            pal,
            "Your library is empty",
            "Click Import folder or drop a folder of samples here. Files stay where they are.",
        );
        return;
    }
    if results.is_empty() {
        let msg = match &local.view {
            View::Pack(_) => {
                "This pack is empty. Add samples from a card's menu or the detail panel."
            }
            View::Favorites => "No favorites yet. Click the star on a card.",
            View::Edited => "No edited versions yet. Use the tools in the detail panel.",
            _ => "Nothing matches. Try clearing a filter.",
        };
        empty(ui, pal, "No samples here", msg);
        return;
    }
    let card_w = s.lib.settings.card_width.clamp(160.0, 320.0);
    let avail = ui.available_width();
    let cols = (((avail + GAP) / (card_w + GAP)).floor() as usize).max(1);
    // Stretch cards to fill the row exactly.
    let w = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32).max(120.0);
    let art_h = (w * 0.62).round();
    let h = art_h + TEXT_H;
    let rows = results.len().div_ceil(cols);
    let player = engine.player();
    let playing_id = player.is_playing().then(|| player.clip_id()).flatten();
    let pos = player.position_sec();
    egui::ScrollArea::vertical()
        .id_salt("smp-grid")
        .auto_shrink([false, false])
        .show_rows(
            ui,
            h + GAP - ui.spacing().item_spacing.y,
            rows,
            |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for c in 0..cols {
                            let Some(&idx) = results.get(row * cols + c) else {
                                break;
                            };
                            let (rect, resp) =
                                ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let smp = s.lib.samples[idx].clone();
                            let is_playing = playing_id.as_deref() == Some(smp.id.as_str());
                            card(
                                ui,
                                rect,
                                &resp,
                                s,
                                local,
                                pal,
                                &smp,
                                is_playing.then_some(pos),
                                art_h,
                            );
                            interact(ui, &resp, rect, s, local, engine, &smp, actions);
                        }
                    });
                    ui.add_space(GAP - ui.spacing().item_spacing.y);
                }
            },
        );
}

fn empty(ui: &mut Ui, pal: &Palette, title: &str, body: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(RichText::new(title).size(22.0).color(pal.text).strong());
        ui.add_space(6.0);
        ui.label(RichText::new(body).size(13.0).color(pal.text_dim));
    });
}

#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut Ui,
    rect: Rect,
    resp: &egui::Response,
    s: &Shared,
    local: &mut UiLocal,
    pal: &Palette,
    smp: &Sample,
    play_pos: Option<f64>,
    art_h: f32,
) {
    let selected = local.selected.as_deref() == Some(smp.id.as_str());
    let hovered = resp.hovered();
    let lift = if hovered { -2.0 } else { 0.0 };
    let rect = rect.translate(vec2(0.0, lift));
    let colors: Vec<Color32> = smp
        .genres
        .iter()
        .map(|g| rgb(s.lib.genre_color(g)))
        .collect();
    let main = colors.first().copied().unwrap_or(pal.accent);
    let p = ui.painter();
    // Shadow and body.
    p.rect_filled(
        rect.translate(vec2(0.0, 4.0)).expand(1.0),
        CornerRadius::same(14),
        Color32::from_black_alpha(if hovered { 90 } else { 50 }),
    );
    p.rect_filled(
        rect,
        CornerRadius::same(12),
        if hovered { pal.card_hover } else { pal.card },
    );
    let art_rect = Rect::from_min_size(rect.min, vec2(rect.width(), art_h));
    let tex = smp
        .art
        .as_deref()
        .and_then(|a| local.textures.get(ui.ctx(), a));
    let radius = CornerRadius {
        nw: 12,
        ne: 12,
        sw: 0,
        se: 0,
    };
    art(p, art_rect, tex.as_ref(), &colors, &smp.name, pal, radius);
    // Waveform over the lower half of the artwork.
    let wave_rect = Rect::from_min_max(
        pos2(art_rect.left() + 10.0, art_rect.bottom() - art_h * 0.46),
        pos2(art_rect.right() - 10.0, art_rect.bottom() - 8.0),
    );
    scrim(
        &p.with_clip_rect(art_rect),
        Rect::from_min_max(
            pos2(art_rect.left(), art_rect.bottom() - art_h * 0.62),
            art_rect.max,
        ),
        Color32::from_black_alpha(200),
    );
    let played = play_pos.map(|t| (t / smp.duration_sec.max(1e-6)) as f32);
    let n = smp.peaks.len();
    waveform(
        p,
        wave_rect,
        n,
        |i| smp.peaks.get(i),
        Color32::from_white_alpha(if played.is_some() { 120 } else { 210 }),
        mix(main, Color32::WHITE, 0.25),
        played,
    );
    if let Some(f) = played {
        let x = wave_rect.left() + wave_rect.width() * f.clamp(0.0, 1.0);
        p.line_segment(
            [
                pos2(x, wave_rect.top() - 2.0),
                pos2(x, wave_rect.bottom() + 2.0),
            ],
            Stroke::new(2.0, Color32::WHITE),
        );
    }
    // Genre stripe along the bottom edge of the artwork.
    if !colors.is_empty() {
        let seg = art_rect.width() / colors.len() as f32;
        for (i, c) in colors.iter().enumerate() {
            let r = Rect::from_min_max(
                pos2(art_rect.left() + seg * i as f32, art_rect.bottom() - 3.0),
                pos2(art_rect.left() + seg * (i + 1) as f32, art_rect.bottom()),
            );
            p.rect_filled(r, 0.0, *c);
        }
    }
    // Badges.
    let mut bx = art_rect.left() + 10.0;
    let mut badge = |text: &str, fill: Color32, fg: Color32| {
        let g = p.layout_no_wrap(text.to_string(), FontId::proportional(10.5), fg);
        let r = Rect::from_min_size(pos2(bx, art_rect.top() + 10.0), g.size() + vec2(12.0, 6.0));
        p.rect_filled(r, CornerRadius::same(255), fill);
        p.galley(r.min + vec2(6.0, 3.0), g, fg);
        bx = r.right() + 5.0;
    };
    badge(
        smp.file_type.label(),
        Color32::from_black_alpha(140),
        Color32::from_white_alpha(220),
    );
    if let Version::Edited { .. } = smp.version {
        badge("EDIT", pal.accent, Color32::WHITE);
    }
    if smp.missing {
        badge("MISSING", pal.danger, Color32::WHITE);
    }
    if play_pos.is_some() {
        badge("PLAYING", Color32::WHITE, Color32::BLACK);
    }
    // Text block.
    let tx = rect.left() + 12.0;
    let mut y = art_rect.bottom() + 9.0;
    let max_chars = ((rect.width() - 24.0) / 7.2) as usize;
    p.text(
        pos2(tx, y),
        Align2::LEFT_TOP,
        ellipsize(&smp.name, max_chars),
        FontId::proportional(14.0),
        pal.text,
    );
    y += 22.0;
    let mut x = tx;
    let mut meta = |text: String, color: Color32| {
        let g = p.layout_no_wrap(text, FontId::proportional(11.5), color);
        let r = Rect::from_min_size(pos2(x, y), g.size() + vec2(10.0, 5.0));
        if r.right() > rect.right() - 8.0 {
            return;
        }
        p.rect_filled(r, CornerRadius::same(5), pal.panel_hi);
        p.galley(r.min + vec2(5.0, 2.5), g, color);
        x = r.right() + 5.0;
    };
    if let Some(b) = smp.bpm {
        meta(bpm(b), pal.text);
    }
    if let Some(k) = &smp.key {
        meta(short_key(k), mix(main, Color32::WHITE, 0.35));
    }
    meta(length(smp.duration_sec), pal.text_dim);
    // Stars on the right of the name line.
    if smp.rating > 0 {
        for i in 0..smp.rating {
            star(
                p,
                pos2(
                    rect.right() - 12.0 - f32::from(i) * 11.0,
                    art_rect.bottom() + 17.0,
                ),
                4.6,
                pal.star,
            );
        }
    }
    // Border.
    let stroke = if selected {
        Stroke::new(2.0, mix(main, Color32::WHITE, 0.2))
    } else if hovered {
        Stroke::new(1.0, pal.line.gamma_multiply(1.6))
    } else {
        Stroke::new(1.0, pal.line)
    };
    p.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Inside);
}

#[allow(clippy::too_many_arguments)]
fn interact(
    ui: &mut Ui,
    resp: &egui::Response,
    rect: Rect,
    s: &mut Shared,
    local: &mut UiLocal,
    engine: &Engine,
    smp: &Sample,
    actions: &mut Vec<Action>,
) {
    let pal = Palette::default();
    let fav_rect = Rect::from_center_size(
        pos2(rect.right() - 22.0, rect.top() + 22.0),
        vec2(26.0, 26.0),
    );
    if (resp.hovered() || smp.favorite)
        && fav_button(
            ui,
            fav_rect,
            smp.favorite,
            &pal,
            egui::Id::unique(("fav", &smp.id)),
        )
    {
        if let Some(x) = s.lib.sample_mut(&smp.id) {
            x.favorite = !x.favorite;
        }
        s.touch();
        return;
    }
    if resp.hovered() && local.hovered.as_deref() != Some(smp.id.as_str()) {
        // Decode on hover so the click plays at once.
        engine.prefetch(&smp.id, &smp.path);
        local.hovered = Some(smp.id.clone());
    }
    if resp.clicked() {
        let player = engine.player();
        let same = player.is_playing() && player.clip_id().as_deref() == Some(smp.id.as_str());
        local.select(Some(smp.id.clone()));
        if same {
            engine.stop();
        } else {
            engine.preview(&smp.id, &smp.path, &smp.fx, 0.0, None);
        }
    }
    if resp.drag_started() {
        let drag = SampleDrag {
            ids: vec![smp.id.clone()],
            paths: vec![smp.path.clone()],
        };
        resp.dnd_set_drag_payload(drag);
        if crate::dragout::supported() {
            actions.push(Action::DragOut {
                paths: vec![smp.path.clone()],
                image: smp.art.clone(),
            });
        }
    }
    resp.context_menu(|ui| context_menu(ui, s, local, engine, smp));
}

fn context_menu(ui: &mut Ui, s: &mut Shared, local: &mut UiLocal, engine: &Engine, smp: &Sample) {
    if ui
        .button(if smp.favorite {
            "Unfavorite"
        } else {
            "Favorite"
        })
        .clicked()
    {
        if let Some(x) = s.lib.sample_mut(&smp.id) {
            x.favorite = !x.favorite;
        }
        s.touch();
        ui.close();
    }
    ui.menu_button("Add to pack", |ui| {
        let packs: Vec<(String, String, bool)> = s
            .lib
            .packs
            .iter()
            .map(|p| (p.id.clone(), p.name.clone(), p.members.contains(&smp.id)))
            .collect();
        if packs.is_empty() {
            ui.label("Create a pack in the sidebar first.");
        }
        for (pid, name, has) in packs {
            if ui
                .add_enabled(
                    !has,
                    egui::Button::new(if has {
                        format!("In pack: {name}")
                    } else {
                        name
                    }),
                )
                .clicked()
            {
                if let Some(p) = s.lib.pack_mut(&pid) {
                    p.members.push(smp.id.clone());
                }
                s.touch();
                ui.close();
            }
        }
    });
    if let View::Pack(pid) = local.view.clone()
        && ui.button("Remove from this pack").clicked()
    {
        if let Some(p) = s.lib.pack_mut(&pid) {
            p.members.retain(|m| *m != smp.id);
        }
        s.touch();
        ui.close();
    }
    if ui.button("Show in folder").clicked() {
        reveal(&smp.path);
        ui.close();
    }
    ui.separator();
    if ui
        .button("Remove from library")
        .on_hover_text("The file itself is kept (edits made here are deleted)")
        .clicked()
    {
        local.confirm_remove = Some(smp.id.clone());
        local.select(Some(smp.id.clone()));
        engine.stop();
        ui.close();
    }
}

/// Opens the file's folder with the file selected.
pub fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}
