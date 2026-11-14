//! Colours for the digital side of the window (header, side view, footer). Each faceplate
//! carries its own colours in `faceplates.rs`.

use egui::{Color32, CornerRadius, Frame, Margin, Stroke};

/// The digital palette: flat slate, blue for BEFORE, orange for AFTER.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Window background.
    pub bg: Color32,
    /// Side-view panel.
    pub panel: Color32,
    /// Cards inside the panel.
    pub card: Color32,
    /// Raised controls.
    pub raised: Color32,
    /// Grid lines.
    pub grid: Color32,
    /// Main text.
    pub text: Color32,
    /// Secondary text.
    pub dim: Color32,
    /// BEFORE (dry input).
    pub before: Color32,
    /// AFTER (wet output).
    pub after: Color32,
    /// Gain reduction.
    pub gr: Color32,
    /// Accent (the sun).
    pub sun: Color32,
    /// Bypass / warning.
    pub warn: Color32,
    /// On / good.
    pub good: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x12, 0x15, 0x1B),
            panel: Color32::from_rgb(0x2A, 0x33, 0x42),
            card: Color32::from_rgb(0x1B, 0x22, 0x2E),
            raised: Color32::from_rgb(0x34, 0x3E, 0x4F),
            grid: Color32::from_rgb(0x2C, 0x37, 0x48),
            text: Color32::from_rgb(0xEC, 0xEF, 0xF4),
            dim: Color32::from_rgb(0x9A, 0xA6, 0xB8),
            before: Color32::from_rgb(0x4C, 0x8D, 0xFF),
            after: Color32::from_rgb(0xFF, 0x8A, 0x2B),
            gr: Color32::from_rgb(0xFF, 0xA9, 0x4D),
            sun: Color32::from_rgb(0xFF, 0xC4, 0x3D),
            warn: Color32::from_rgb(0xFF, 0x55, 0x5E),
            good: Color32::from_rgb(0x5E, 0xD0, 0x8A),
        }
    }
}

/// Applies the dark style.
pub fn apply(ctx: &egui::Context) {
    let pal = Palette::default();
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = pal.bg;
        v.window_fill = pal.card;
        v.extreme_bg_color = pal.card;
        v.override_text_color = Some(pal.text);
        v.widgets.inactive.weak_bg_fill = pal.raised;
        v.widgets.inactive.bg_fill = pal.raised;
        v.widgets.hovered.weak_bg_fill = pal.grid;
        v.widgets.active.weak_bg_fill = pal.grid;
        v.selection.bg_fill = pal.before;
        v.selection.stroke = Stroke::new(1.0, pal.text);
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
    });
}

/// Frame for the header and footer bars.
pub fn bar_frame(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.bg)
        .inner_margin(Margin::symmetric(10, 6))
}

/// Frame for the body.
pub fn body_frame(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.bg)
        .inner_margin(Margin::symmetric(10, 4))
}

/// Rounded card.
pub fn card(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.card)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(8))
}
