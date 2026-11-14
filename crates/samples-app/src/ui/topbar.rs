//! Search and filters.

use egui::{RichText, Ui};
use solplugs_samples_core::filter::Sort;
use solplugs_samples_core::keys::Key;

use super::theme::Palette;
use super::{UiLocal, View};
use crate::engine::{Engine, Shared};

fn sort_name(s: Sort) -> &'static str {
    match s {
        Sort::Newest => "Newest",
        Sort::Name => "Name",
        Sort::Bpm => "BPM",
        Sort::Key => "Key",
        Sort::Length => "Length",
        Sort::Rating => "Rating",
    }
}

pub fn show(ui: &mut Ui, s: &mut Shared, local: &mut UiLocal, pal: &Palette, _engine: &Engine) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("SolPlugs").size(13.0).color(pal.text_dim).strong());
        ui.label(RichText::new("Samples").size(18.0).color(pal.text).strong());
        ui.add_space(12.0);
        let search = ui.add(
            egui::TextEdit::singleline(&mut local.query.text)
                .hint_text("Search names, tags, genres, keys…")
                .desired_width(260.0),
        );
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            search.request_focus();
        }
        ui.add_space(6.0);
        ui.label(RichText::new("BPM").color(pal.text_dim));
        let mut lo = local.query.bpm_min.unwrap_or(0.0);
        let mut hi = local.query.bpm_max.unwrap_or(0.0);
        let r1 = ui.add(egui::DragValue::new(&mut lo).range(0.0..=300.0).speed(1.0).custom_formatter(|v, _| if v <= 0.0 { "any".into() } else { format!("{v:.0}") }));
        ui.label("–");
        let r2 = ui.add(egui::DragValue::new(&mut hi).range(0.0..=300.0).speed(1.0).custom_formatter(|v, _| if v <= 0.0 { "any".into() } else { format!("{v:.0}") }));
        if r1.changed() || r2.changed() {
            local.query.bpm_min = (lo > 0.0).then_some(lo);
            local.query.bpm_max = (hi > 0.0).then_some(hi);
        }
        let key_label = local.query.key.clone().unwrap_or_else(|| "Any key".into());
        egui::ComboBox::from_id_salt("smp-key").selected_text(key_label).width(110.0).show_ui(ui, |ui| {
            if ui.selectable_label(local.query.key.is_none(), "Any key").clicked() {
                local.query.key = None;
            }
            for k in Key::all() {
                let n = k.name();
                if ui.selectable_label(local.query.key.as_deref() == Some(n.as_str()), &n).clicked() {
                    local.query.key = Some(n);
                }
            }
        });
        let rating = if local.query.min_rating == 0 { "Any rating".to_string() } else { format!("{}+ stars", local.query.min_rating) };
        egui::ComboBox::from_id_salt("smp-rating").selected_text(rating).width(96.0).show_ui(ui, |ui| {
            for r in 0..=5u8 {
                let t = if r == 0 { "Any rating".to_string() } else { format!("{r}+ stars") };
                if ui.selectable_label(local.query.min_rating == r, t).clicked() {
                    local.query.min_rating = r;
                }
            }
        });
        egui::ComboBox::from_id_salt("smp-sort")
            .selected_text(format!("Sort: {}", sort_name(local.query.sort)))
            .width(110.0)
            .show_ui(ui, |ui| {
                for so in [Sort::Newest, Sort::Name, Sort::Bpm, Sort::Key, Sort::Length, Sort::Rating] {
                    let label = if so == Sort::Newest && matches!(local.view, View::Pack(_)) { "Pack order" } else { sort_name(so) };
                    if ui.selectable_label(local.query.sort == so, label).clicked() {
                        local.query.sort = so;
                    }
                }
            });
        let filtered = local.query.text.trim().len()
            + local.query.tags.len()
            + local.query.genres.len()
            + usize::from(local.query.bpm_min.is_some() || local.query.bpm_max.is_some())
            + usize::from(local.query.key.is_some())
            + usize::from(local.query.min_rating > 0);
        if filtered > 0 && ui.button("Clear filters").clicked() {
            let sort = local.query.sort;
            local.query = Default::default();
            local.query.sort = sort;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut w = s.lib.settings.card_width;
            if ui
                .add(egui::Slider::new(&mut w, 160.0..=320.0).show_value(false))
                .on_hover_text("Card size")
                .changed()
            {
                s.lib.settings.card_width = w;
                s.touch();
            }
            ui.label(RichText::new("Size").color(pal.text_dim));
        });
    });
}
