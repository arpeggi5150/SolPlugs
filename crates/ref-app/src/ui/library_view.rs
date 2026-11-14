//! LIBRARY: the saved references (up to 60), the comparison group, saved groups, import/export.

use egui::{RichText, Ui};
use solplugs_ref_io::library::{MAX_REFERENCES, Origin};

use super::UiLocal;
use super::theme::Palette;
use super::widgets::{duration, ellipsize};
use crate::state::{AppState, Command};

fn pick_audio_files() -> Option<Vec<std::path::PathBuf>> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "flac", "mp3"])
            .set_title("Add references")
            .pick_files()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

fn pick_json(save: bool) -> Option<std::path::PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let d = rfd::FileDialog::new().add_filter("SolPlugs reference library", &["json"]);
        if save {
            d.set_file_name("solplugs-references.json").save_file()
        } else {
            d.pick_file()
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = save;
        None
    }
}

pub fn show(
    ui: &mut Ui,
    s: &AppState,
    pal: &Palette,
    local: &mut UiLocal,
    cmds: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "LIBRARY  {}/{MAX_REFERENCES}",
                s.library.references.len()
            ))
            .strong()
            .size(12.0),
        );
        if ui.button("Add audio files…").clicked()
            && let Some(paths) = pick_audio_files()
        {
            cmds.push(Command::ImportFiles(paths));
        }
        ui.add(
            egui::TextEdit::singleline(&mut local.path_input)
                .hint_text("…or paste a file path")
                .desired_width(260.0),
        );
        if ui.button("Add").clicked() && !local.path_input.trim().is_empty() {
            let p = local.path_input.trim().trim_matches('"').to_string();
            cmds.push(Command::ImportFiles(vec![p.into()]));
            local.path_input.clear();
        }
        ui.separator();
        if ui.button("Export library…").clicked()
            && let Some(p) = pick_json(true)
        {
            cmds.push(Command::ExportLibrary(p));
        }
        if ui.button("Import library…").clicked()
            && let Some(p) = pick_json(false)
        {
            cmds.push(Command::ImportLibrary(p));
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("GROUP").strong().size(12.0));
        ui.label(
            RichText::new("Tick references to compare against them together.")
                .color(pal.text_dim)
                .size(11.0),
        );
        ui.separator();
        egui::ComboBox::from_id_salt("load-group")
            .selected_text("Load saved group")
            .show_ui(ui, |ui| {
                for g in &s.library.groups {
                    if ui
                        .selectable_label(false, format!("{} ({})", g.name, g.members.len()))
                        .clicked()
                    {
                        cmds.push(Command::LoadGroup(g.name.clone()));
                    }
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut local.group_name)
                .hint_text("Group name")
                .desired_width(140.0),
        );
        if ui.button("Save group").clicked() {
            cmds.push(Command::SaveGroup(local.group_name.clone()));
        }
        if !local.group_name.is_empty()
            && s.library.groups.iter().any(|g| g.name == local.group_name)
            && ui.button("Delete group").clicked()
        {
            cmds.push(Command::DeleteGroup(local.group_name.clone()));
        }
    });
    ui.add_space(4.0);
    let refs = s.all_refs();
    if refs.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(
                    "No references yet. Add audio files, search online, or capture with SolRoute.",
                )
                .color(pal.text_dim)
                .size(14.0),
            );
        });
        return;
    }
    egui::ScrollArea::vertical().id_salt("lib").show(ui, |ui| {
        egui::Grid::new("lib-grid")
            .num_columns(8)
            .striped(true)
            .spacing(egui::vec2(12.0, 6.0))
            .show(ui, |ui| {
                for h in [
                    "Group", "", "Title", "Source", "Loudness", "Peak", "Length", "",
                ] {
                    ui.label(RichText::new(h).size(11.0).color(pal.text_dim));
                }
                ui.end_row();
                for r in &refs {
                    let mut in_group = s.group.contains(&r.id);
                    if ui.checkbox(&mut in_group, "").changed() {
                        cmds.push(Command::ToggleInGroup(r.id.clone()));
                    }
                    let active = s.active.as_ref() == Some(&r.id);
                    if ui
                        .selectable_label(
                            active,
                            if active {
                                "▶ playing"
                            } else {
                                "▶ play on B"
                            },
                        )
                        .clicked()
                    {
                        cmds.push(Command::SetActive(r.id.clone()));
                    }
                    match &mut local.rename {
                        Some((id, title, artist)) if *id == r.id => {
                            let mut done = false;
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(title).desired_width(160.0));
                                ui.add(
                                    egui::TextEdit::singleline(artist)
                                        .desired_width(100.0)
                                        .hint_text("Artist"),
                                );
                                if ui.button("OK").clicked() {
                                    cmds.push(Command::Rename {
                                        id: id.clone(),
                                        title: title.clone(),
                                        artist: artist.clone(),
                                    });
                                    done = true;
                                }
                            });
                            if done {
                                local.rename = None;
                            }
                        }
                        _ => {
                            let label = if r.artist.is_empty() {
                                ellipsize(&r.title, 48)
                            } else {
                                format!(
                                    "{} — {}",
                                    ellipsize(&r.title, 36),
                                    ellipsize(&r.artist, 24)
                                )
                            };
                            if ui
                                .add(
                                    egui::Label::new(RichText::new(label).color(pal.text))
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Click to rename")
                                .clicked()
                                && r.in_library
                            {
                                local.rename =
                                    Some((r.id.clone(), r.title.clone(), r.artist.clone()));
                            }
                        }
                    }
                    let source = match &r.origin {
                        Origin::File { .. } => "File",
                        Origin::Deezer { .. } => "Deezer preview",
                        Origin::Capture => "SolRoute",
                    };
                    ui.label(RichText::new(source).size(11.0).color(pal.text_dim));
                    ui.label(
                        RichText::new(
                            r.profile
                                .integrated_lufs
                                .map_or("—".into(), |l| format!("{l:.1} LUFS")),
                        )
                        .monospace(),
                    );
                    ui.label(
                        RichText::new(format!("{:.1} dBTP", r.profile.true_peak_db))
                            .monospace()
                            .color(pal.text_dim),
                    );
                    ui.label(
                        RichText::new(duration(r.profile.duration_secs))
                            .monospace()
                            .color(pal.text_dim),
                    );
                    ui.horizontal(|ui| {
                        if !r.in_library
                            && ui
                                .button("Save")
                                .on_hover_text(
                                    "Keep its analysis in the library (audio is never saved)",
                                )
                                .clicked()
                        {
                            cmds.push(Command::SaveSession(r.id.clone()));
                        }
                        if ui.button("Remove").clicked() {
                            cmds.push(Command::Remove(r.id.clone()));
                        }
                    });
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Display tilt").size(11.0).color(pal.text_dim));
            let mut t = s.library.settings.display_tilt_db_per_oct;
            if ui
                .add(
                    egui::Slider::new(&mut t, 0.0..=6.0)
                        .step_by(0.5)
                        .suffix(" dB/oct"),
                )
                .on_hover_text(
                    "Tilts tonal-balance curves around 1 kHz. At 0, pink noise reads flat. Display only.",
                )
                .changed()
            {
                cmds.push(Command::SetTilt(t));
            }
        });
    });
}
