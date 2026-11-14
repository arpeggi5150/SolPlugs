//! The sidebar: library sections, packs, genres and tags.

use egui::{CornerRadius, RichText, Sense, Ui, vec2};
use solplugs_samples_core::model::{Pack, now_unix};

use super::theme::{Palette, rgb};
use super::widgets::{art, ellipsize, genre_chip, heading};
use super::{Action, UiLocal, View};
use crate::engine::{Engine, Shared};
use crate::worker::Cmd;

fn nav(ui: &mut Ui, local: &mut UiLocal, view: View, label: &str, count: usize, pal: &Palette) {
    let on = local.view == view;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CornerRadius::same(7), pal.accent.gamma_multiply(0.25));
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(7), pal.panel_hi);
    }
    p.text(
        rect.left_center() + vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        if on { pal.text } else { pal.text_dim },
    );
    p.text(
        rect.right_center() - vec2(10.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.5),
        pal.text_faint,
    );
    if resp.clicked() {
        local.view = view;
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
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            let busy = s.import.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new(RichText::new("+ Import folder").strong()).fill(pal.accent.gamma_multiply(0.8)))
                .on_hover_text("Scan a folder of samples. Files stay where they are. You can also drop folders on the window.")
                .clicked()
                && super::has_dialogs() {
                    actions.push(Action::PickImport);
                }
            if ui.add_enabled(!busy, egui::Button::new("Rescan")).on_hover_text("Rescan imported folders for new or missing files").clicked() {
                engine.send(Cmd::Rescan);
            }
        });
        if !super::has_dialogs() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut local.path_input).hint_text("Folder path").desired_width(140.0));
                if ui.button("Add").clicked() && !local.path_input.trim().is_empty() {
                    engine.send(Cmd::Import(vec![local.path_input.trim().trim_matches('"').into()]));
                    local.path_input.clear();
                }
            });
        }
        ui.add_space(10.0);
        heading(ui, "LIBRARY", pal);
        let total = s.lib.samples.len();
        let favs = s.lib.samples.iter().filter(|x| x.favorite).count();
        let edited = s.lib.samples.iter().filter(|x| x.version.is_edited()).count();
        nav(ui, local, View::All, "All samples", total, pal);
        nav(ui, local, View::Favorites, "Favorites", favs, pal);
        nav(ui, local, View::Raws, "Raws", total - edited, pal);
        nav(ui, local, View::Edited, "Edited", edited, pal);

        ui.add_space(12.0);
        heading(ui, "PACKS", pal);
        let packs: Vec<(String, String, Option<std::path::PathBuf>, usize, Vec<String>)> = s
            .lib
            .packs
            .iter()
            .map(|p| (p.id.clone(), p.name.clone(), p.cover.clone(), p.members.len(), p.genres.clone()))
            .collect();
        for (id, name, cover, n, genres) in packs {
            let on = local.view == View::Pack(id.clone());
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
            let p = ui.painter();
            if on {
                p.rect_filled(rect, CornerRadius::same(8), pal.accent.gamma_multiply(0.25));
            } else if resp.hovered() {
                p.rect_filled(rect, CornerRadius::same(8), pal.panel_hi);
            }
            let thumb = egui::Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(32.0, 32.0));
            let tex = cover.as_deref().and_then(|c| local.textures.get(ui.ctx(), c));
            let colors: Vec<_> = genres.iter().map(|g| rgb(s.lib.genre_color(g))).collect();
            art(ui.painter(), thumb, tex.as_ref(), &colors, &name, pal, CornerRadius::same(6));
            let p = ui.painter();
            p.text(thumb.right_center() + vec2(8.0, -7.0), egui::Align2::LEFT_CENTER, ellipsize(&name, 22), egui::FontId::proportional(13.0), pal.text);
            p.text(thumb.right_center() + vec2(8.0, 8.0), egui::Align2::LEFT_CENTER, format!("{n} samples"), egui::FontId::proportional(11.0), pal.text_faint);
            // Drop a dragged card on a pack to add it.
            if let Some(drag) = resp.dnd_release_payload::<crate::dragout::SampleDrag>() {
                if let Some(pk) = s.lib.pack_mut(&id) {
                    for sid in &drag.ids {
                        if !pk.members.contains(sid) {
                            pk.members.push(sid.clone());
                        }
                    }
                }
                s.touch();
            }
            if resp.clicked() {
                local.view = View::Pack(id.clone());
            }
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut local.new_pack).hint_text("New pack name").desired_width(ui.available_width() - 40.0));
            let go = ui.button("+").on_hover_text("Create the pack").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go && !local.new_pack.trim().is_empty() {
                let id = s.lib.new_id();
                s.lib.packs.push(Pack {
                    id: id.clone(),
                    name: local.new_pack.trim().to_string(),
                    created_unix: now_unix(),
                    ..Pack::default()
                });
                local.new_pack.clear();
                local.view = View::Pack(id);
                s.touch();
            }
        });

        ui.add_space(12.0);
        heading(ui, "GENRES", pal);
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for smp in &s.lib.samples {
            for g in &smp.genres {
                *counts.entry(g.clone()).or_default() += 1;
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
            for (g, n) in &counts {
                let on = local.query.genres.iter().any(|x| x.eq_ignore_ascii_case(g));
                if genre_chip(ui, &format!("{g} {n}"), rgb(s.lib.genre_color(g)), on).clicked() {
                    if on {
                        local.query.genres.retain(|x| !x.eq_ignore_ascii_case(g));
                    } else {
                        local.query.genres.push(g.clone());
                    }
                }
            }
            if counts.is_empty() {
                ui.label(RichText::new("Genres from tags and folder names show here.").size(11.0).color(pal.text_faint));
            }
        });

        ui.add_space(12.0);
        heading(ui, "TAGS", pal);
        let tags = s.lib.tag_counts();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
            for (t, n) in tags.iter().take(60) {
                let on = local.query.tags.iter().any(|x| x.eq_ignore_ascii_case(t));
                let fill = if on { pal.accent2 } else { pal.panel_hi };
                let fg = if on { egui::Color32::from_rgb(0x08, 0x10, 0x12) } else { pal.text_dim };
                if super::widgets::chip(ui, &format!("#{t} {n}"), fill, fg).clicked() {
                    if on {
                        local.query.tags.retain(|x| !x.eq_ignore_ascii_case(t));
                    } else {
                        local.query.tags.push(t.clone());
                    }
                }
            }
            if tags.is_empty() {
                ui.label(RichText::new("Add tags to samples in the detail panel.").size(11.0).color(pal.text_faint));
            }
        });
    });
}
