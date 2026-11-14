//! The look: dark like Solara, with each genre's colour carrying the cards.

use egui::{Color32, CornerRadius, Frame, Margin, Stroke};

/// The palette.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub panel: Color32,
    pub panel_hi: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent2: Color32,
    pub star: Color32,
    pub danger: Color32,
    pub ok: Color32,
    pub wave: Color32,
    pub selection: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x0D, 0x0F, 0x14),
            sidebar: Color32::from_rgb(0x12, 0x15, 0x1B),
            panel: Color32::from_rgb(0x15, 0x18, 0x1F),
            panel_hi: Color32::from_rgb(0x1E, 0x22, 0x2B),
            card: Color32::from_rgb(0x17, 0x1A, 0x22),
            card_hover: Color32::from_rgb(0x1E, 0x22, 0x2C),
            line: Color32::from_rgb(0x28, 0x2D, 0x38),
            text: Color32::from_rgb(0xEC, 0xEE, 0xF3),
            text_dim: Color32::from_rgb(0x98, 0x9F, 0xAE),
            text_faint: Color32::from_rgb(0x5E, 0x65, 0x73),
            accent: Color32::from_rgb(0x7C, 0x6C, 0xFF),
            accent2: Color32::from_rgb(0x2E, 0xD3, 0xC0),
            star: Color32::from_rgb(0xFF, 0xC5, 0x3D),
            danger: Color32::from_rgb(0xFF, 0x5D, 0x73),
            ok: Color32::from_rgb(0x6B, 0xD3, 0x8A),
            wave: Color32::from_rgb(0xF2, 0xF4, 0xF8),
            selection: Color32::from_rgba_unmultiplied(0x7C, 0x6C, 0xFF, 70),
        }
    }
}

/// sRGB triple to a colour.
#[must_use]
pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// `a` mixed toward `b` by `t` (0..1).
#[must_use]
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
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
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.line);
        v.widgets.inactive.bg_fill = pal.panel_hi;
        v.widgets.inactive.weak_bg_fill = pal.panel_hi;
        v.widgets.inactive.corner_radius = CornerRadius::same(6);
        v.widgets.hovered.weak_bg_fill = pal.line;
        v.widgets.hovered.corner_radius = CornerRadius::same(6);
        v.widgets.active.corner_radius = CornerRadius::same(6);
        v.selection.bg_fill = pal.accent.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, pal.accent);
        v.hyperlink_color = pal.accent2;
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.slider_trailing_fill = true;
        v.widgets.inactive.bg_fill = pal.line;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.slider_width = 150.0;
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 24.0;
    });
}

/// Frame for the top bar.
pub fn bar(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.panel)
        .inner_margin(Margin::symmetric(14, 10))
}

/// Frame for the sidebar.
pub fn sidebar(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.sidebar)
        .inner_margin(Margin::symmetric(12, 12))
}

/// Frame for the detail panel.
pub fn detail(pal: &Palette) -> Frame {
    Frame::new().fill(pal.panel).inner_margin(Margin::same(14))
}

/// Frame for the grid.
pub fn body(pal: &Palette) -> Frame {
    Frame::new().fill(pal.bg).inner_margin(Margin::same(14))
}

/// A section block inside the detail panel.
pub fn section(pal: &Palette) -> Frame {
    Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.line))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
}
