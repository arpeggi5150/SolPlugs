//! SEARCH: find any song and load its 30-second Deezer preview as a reference.

use egui::{RichText, Ui};

use super::UiLocal;
use super::theme::{Palette, card};
use super::widgets::duration;
use crate::state::{AppState, Command};

pub fn show(
    ui: &mut Ui,
    s: &AppState,
    pal: &Palette,
    local: &mut UiLocal,
    cmds: &mut Vec<Command>,
) {
    if !cfg!(feature = "deezer") {
        ui.label("This build has no online search.");
        return;
    }
    if !s.library.settings.online_search {
        card(pal).show(ui, |ui| {
            ui.set_max_width(560.0);
            ui.label(RichText::new("Online search is off").strong().size(15.0));
            ui.label("Search any song and load its 30-second preview from Deezer as a reference. Previews stream into memory and are never saved to disk; when you save one to the library, only its analysis is kept.");
            ui.label(RichText::new("Deezer's API terms allow non-commercial use. Turn this on only if your use fits them.").color(pal.text_dim));
            ui.hyperlink_to("Deezer API terms", "https://developers.deezer.com/termsofuse");
            if ui.button("Turn on online search").clicked() {
                cmds.push(Command::SetOnlineSearch(true));
            }
        });
        return;
    }
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut local.search_input)
                .hint_text("Artist, song…")
                .desired_width(360.0),
        );
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Search").clicked() || enter) && !local.search_input.trim().is_empty() {
            cmds.push(Command::Search(local.search_input.clone()));
        }
        if s.search.busy {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Turn off").clicked() {
                cmds.push(Command::SetOnlineSearch(false));
            }
            ui.label(
                RichText::new("Previews from Deezer")
                    .size(11.0)
                    .color(pal.text_dim),
            );
        });
    });
    if let Some(e) = &s.search.error {
        ui.label(RichText::new(e).color(pal.above));
    }
    egui::ScrollArea::vertical()
        .id_salt("search")
        .show(ui, |ui| {
            egui::Grid::new("hits")
                .num_columns(4)
                .striped(true)
                .spacing(egui::vec2(14.0, 6.0))
                .show(ui, |ui| {
                    for h in &s.search.results {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&h.title).color(pal.text));
                            ui.label(
                                RichText::new(format!("{} · {}", h.artist, h.album))
                                    .size(11.0)
                                    .color(pal.text_dim),
                            );
                        });
                        ui.label(
                            RichText::new(duration(f64::from(h.duration_secs)))
                                .monospace()
                                .color(pal.text_dim),
                        );
                        if ui.button("Load 30 s preview").clicked() {
                            cmds.push(Command::Preview(h.clone()));
                        }
                        if h.link.starts_with("https://") {
                            ui.hyperlink_to(RichText::new("Open on Deezer").size(11.0), &h.link);
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
        });
}
