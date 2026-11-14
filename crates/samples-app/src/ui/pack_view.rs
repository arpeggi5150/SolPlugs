//! The header above the grid when a pack is open: cover, name, description, tags, export.

use egui::{Color32, CornerRadius, FontId, RichText, Sense, Ui, vec2};

use super::theme::{Palette, rgb, section};
use super::widgets::{art, chip, genre_chip};
use super::{Action, UiLocal, View};
use crate::engine::{Engine, Shared};

pub fn show(ui: &mut Ui, s: &mut Shared, local: &mut UiLocal, pal: &Palette, _engine: &Engine, id: &str, actions: &mut Vec<Action>) {
    let Some(pack) = s.lib.pack(id).cloned() else { return };
    let raws = pack.members.iter().filter_map(|m| s.lib.sample(m)).filter(|x| !x.version.is_edited()).count();
    let edits = pack.members.len() - raws;
    section(pal).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(132.0, 132.0), Sense::click());
            let colors: Vec<Color32> = pack.genres.iter().map(|g| rgb(s.lib.genre_color(g))).collect();
            let tex = pack.cover.as_deref().and_then(|c| local.textures.get(ui.ctx(), c));
            art(ui.painter(), rect, tex.as_ref(), &colors, &pack.name, pal, CornerRadius::same(12));
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(12), Color32::from_black_alpha(120));
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Change cover", FontId::proportional(13.0), pal.text);
            }
            if resp.clicked() {
                if let Some(c) = &pack.cover {
                    local.textures.forget(c);
                }
                actions.push(Action::PickCover(pack.id.clone()));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("PACK").size(11.0).strong().color(pal.text_dim));
                let mut name = pack.name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).font(FontId::proportional(24.0)).desired_width(ui.available_width().min(520.0))).changed() {
                    if let Some(p) = s.lib.pack_mut(id) {
                        p.name = name;
                    }
                    s.touch();
                }
                let mut desc = pack.description.clone();
                if ui
                    .add(egui::TextEdit::multiline(&mut desc).hint_text("Describe the pack…").desired_rows(2).desired_width(ui.available_width().min(520.0)))
                    .changed()
                {
                    if let Some(p) = s.lib.pack_mut(id) {
                        p.description = desc;
                    }
                    s.touch();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
                    ui.label(RichText::new(format!("{raws} raw · {edits} edited")).color(pal.text_dim));
                    for g in &pack.genres {
                        if genre_chip(ui, &format!("{g}  ×"), rgb(s.lib.genre_color(g)), true).clicked() {
                            if let Some(p) = s.lib.pack_mut(id) {
                                p.genres.retain(|x| x != g);
                            }
                            s.touch();
                        }
                    }
                    egui::ComboBox::from_id_salt("pack-genre").selected_text("+ genre").width(90.0).show_ui(ui, |ui| {
                        let names: Vec<String> = s.lib.genres.iter().map(|g| g.name.clone()).collect();
                        for n in names {
                            if !pack.genres.contains(&n) && ui.selectable_label(false, &n).clicked() {
                                if let Some(p) = s.lib.pack_mut(id) {
                                    p.genres.push(n);
                                }
                                s.touch();
                            }
                        }
                    });
                    for t in &pack.tags {
                        if chip(ui, &format!("#{t}  ×"), pal.accent2.gamma_multiply(0.25), pal.text).clicked() {
                            if let Some(p) = s.lib.pack_mut(id) {
                                p.tags.retain(|x| x != t);
                            }
                            s.touch();
                        }
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut local.new_tag).hint_text("pack tag").desired_width(90.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let t = local.new_tag.trim().trim_start_matches('#').to_lowercase();
                        if !t.is_empty()
                            && let Some(p) = s.lib.pack_mut(id)
                            && !p.tags.contains(&t)
                        {
                            p.tags.push(t);
                        }
                        local.new_tag.clear();
                        s.touch();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!pack.members.is_empty() && super::has_dialogs(), egui::Button::new("Export pack…")).on_hover_text("Copies the samples (raws and edits) and pack.json into a new folder").clicked() {
                        actions.push(Action::PickExport(pack.id.clone()));
                    }
                    if local.confirm_pack_delete.as_deref() == Some(id) {
                        ui.label(RichText::new("Delete this pack? Its samples stay in the library.").color(pal.danger));
                        if ui.button("Delete").clicked() {
                            s.lib.packs.retain(|p| p.id != id);
                            s.touch();
                            local.view = View::All;
                            local.confirm_pack_delete = None;
                        }
                        if ui.button("Keep").clicked() {
                            local.confirm_pack_delete = None;
                        }
                    } else if ui.button("Delete pack").clicked() {
                        local.confirm_pack_delete = Some(id.to_string());
                    }
                });
            });
        });
    });
    ui.add_space(12.0);
}
