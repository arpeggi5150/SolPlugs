//! Colours and frames. Dark, one colour per meaning: input pitch blue, corrected pitch
//! amber, notes violet, selection white.

use egui::{Color32, CornerRadius, Frame, Margin, Stroke};

/// The palette.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Window background.
    pub bg: Color32,
    /// Card background.
    pub panel: Color32,
    /// Raised controls.
    pub panel_hi: Color32,
    /// Grid lines.
    pub grid: Color32,
    /// Scale-note rows.
    pub row_in_scale: Color32,
    /// Main text.
    pub text: Color32,
    /// Secondary text.
    pub text_dim: Color32,
    /// Detected (input) pitch.
    pub input: Color32,
    /// Corrected (output) pitch.
    pub output: Color32,
    /// Note blobs.
    pub note: Color32,
    /// Selected note.
    pub selected: Color32,
    /// Capture / record.
    pub record: Color32,
    /// In tune.
    pub good: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x0F, 0x11, 0x15),
            panel: Color32::from_rgb(0x17, 0x1A, 0x20),
            panel_hi: Color32::from_rgb(0x22, 0x26, 0x2E),
            grid: Color32::from_rgb(0x2A, 0x2F, 0x38),
            row_in_scale: Color32::from_rgb(0x1C, 0x20, 0x27),
            text: Color32::from_rgb(0xE6, 0xE9, 0xEF),
            text_dim: Color32::from_rgb(0x8A, 0x92, 0xA0),
            input: Color32::from_rgb(0x5A, 0xB4, 0xFF),
            output: Color32::from_rgb(0xFF, 0xB5, 0x47),
            note: Color32::from_rgb(0x9B, 0x7B, 0xFF),
            selected: Color32::from_rgb(0xF2, 0xF4, 0xF8),
            record: Color32::from_rgb(0xFF, 0x4D, 0x5E),
            good: Color32::from_rgb(0x6B, 0xD3, 0x8A),
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
        v.window_fill = pal.panel;
        v.extreme_bg_color = pal.bg;
        v.faint_bg_color = pal.panel;
        v.widgets.noninteractive.bg_fill = pal.panel;
        v.widgets.inactive.bg_fill = pal.panel_hi;
        v.widgets.inactive.weak_bg_fill = pal.panel_hi;
        v.widgets.hovered.weak_bg_fill = pal.grid;
        v.selection.bg_fill = pal.output.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, pal.output);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

/// Frame for the top bar.
pub fn bar_frame(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.panel)
        .inner_margin(Margin::symmetric(12, 8))
}

/// Frame for the main area.
pub fn body_frame(pal: &Palette) -> Frame {
    Frame::new().fill(pal.bg).inner_margin(Margin::same(12))
}

/// A raised card.
pub fn card(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.panel)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .stroke(Stroke::new(1.0, pal.grid))
}
