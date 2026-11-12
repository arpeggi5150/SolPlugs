//! Colours and frames. Dark, high-contrast, one colour per meaning.

use egui::{Color32, CornerRadius, Frame, Margin, Stroke};

/// The palette. Mix is teal, reference amber; above-range red, below-range blue, in-range green.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub grid: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub mix: Color32,
    pub reference: Color32,
    pub group: Color32,
    pub above: Color32,
    pub below: Color32,
    pub within: Color32,
    pub sub: Color32,
    pub kick: Color32,
    pub snare: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x10, 0x12, 0x16),
            panel: Color32::from_rgb(0x17, 0x1A, 0x20),
            panel_hi: Color32::from_rgb(0x21, 0x25, 0x2D),
            grid: Color32::from_rgb(0x2A, 0x2F, 0x38),
            text: Color32::from_rgb(0xE6, 0xE9, 0xEF),
            text_dim: Color32::from_rgb(0x8A, 0x92, 0xA0),
            mix: Color32::from_rgb(0x2E, 0xD3, 0xC0),
            reference: Color32::from_rgb(0xFF, 0xB5, 0x47),
            group: Color32::from_rgb(0x9A, 0xA3, 0xB5),
            above: Color32::from_rgb(0xFF, 0x5D, 0x73),
            below: Color32::from_rgb(0x4E, 0xA8, 0xFF),
            within: Color32::from_rgb(0x6B, 0xD3, 0x8A),
            sub: Color32::from_rgb(0x9B, 0x7B, 0xFF),
            kick: Color32::from_rgb(0xFF, 0x8A, 0x3D),
            snare: Color32::from_rgb(0x3D, 0xD6, 0xF5),
        }
    }
}

impl Palette {
    /// The colour of a PARTS element.
    #[must_use]
    pub fn part(&self, k: solplugs_ref_core::parts::PartKind) -> Color32 {
        match k {
            solplugs_ref_core::parts::PartKind::Sub => self.sub,
            solplugs_ref_core::parts::PartKind::Kick => self.kick,
            solplugs_ref_core::parts::PartKind::Snare => self.snare,
        }
    }

    /// Colour for a signed distance outside a range.
    #[must_use]
    pub fn verdict(&self, outside: f64) -> Color32 {
        if outside > 0.0 {
            self.above
        } else if outside < 0.0 {
            self.below
        } else {
            self.within
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
        v.selection.bg_fill = pal.mix.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, pal.mix);
        v.hyperlink_color = pal.mix;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

/// Frame for the top and bottom bars.
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
