//! Small drawing helpers: artwork with a genre-gradient fallback, waveforms, chips, stars.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    TextureHandle, Ui, pos2, vec2,
};

use super::theme::{Palette, mix};

/// "0:04.2" under a minute, "1:23" above.
#[must_use]
pub fn length(sec: f64) -> String {
    if sec < 60.0 {
        format!("0:{sec:04.1}")
    } else {
        let s = sec.round() as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "128 BPM" (one decimal only when it matters).
#[must_use]
pub fn bpm(b: f64) -> String {
    if (b - b.round()).abs() < 0.05 {
        format!("{} BPM", b.round())
    } else {
        format!("{b:.1} BPM")
    }
}

/// Short key for cards ("Am", "F#").
#[must_use]
pub fn short_key(k: &str) -> String {
    solplugs_samples_core::keys::parse(k).map_or_else(|| k.to_string(), |k| k.short())
}

/// A four-corner gradient fill.
pub fn gradient(
    painter: &egui::Painter,
    rect: Rect,
    tl: Color32,
    tr: Color32,
    bl: Color32,
    br: Color32,
) {
    let mut m = Mesh::default();
    let i = m.vertices.len() as u32;
    for (p, c) in [
        (rect.left_top(), tl),
        (rect.right_top(), tr),
        (rect.left_bottom(), bl),
        (rect.right_bottom(), br),
    ] {
        m.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color: c,
        });
    }
    m.indices
        .extend_from_slice(&[i, i + 1, i + 2, i + 1, i + 3, i + 2]);
    painter.add(m);
}

/// A vertical fade from transparent to `c` (used as a scrim under the waveform).
pub fn scrim(painter: &egui::Painter, rect: Rect, c: Color32) {
    gradient(
        painter,
        rect,
        Color32::TRANSPARENT,
        Color32::TRANSPARENT,
        c,
        c,
    );
}

/// Artwork, or a gradient in the sample's genre colours with its initial when there's none.
pub fn art(
    painter: &egui::Painter,
    rect: Rect,
    tex: Option<&TextureHandle>,
    colors: &[Color32],
    name: &str,
    pal: &Palette,
    radius: CornerRadius,
) {
    if let Some(t) = tex {
        let size = t.size_vec2();
        // Cover-fit: crop the longer side.
        let (ra, ri) = (rect.width() / rect.height(), size.x / size.y.max(1.0));
        let uv = if ri > ra {
            let w = ra / ri;
            Rect::from_min_max(pos2(0.5 - w / 2.0, 0.0), pos2(0.5 + w / 2.0, 1.0))
        } else {
            let h = ri / ra;
            Rect::from_min_max(pos2(0.0, 0.5 - h / 2.0), pos2(1.0, 0.5 + h / 2.0))
        };
        painter.add(
            egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(t.id(), uv),
        );
        return;
    }
    let a = colors.first().copied().unwrap_or(pal.accent);
    let b = colors
        .get(1)
        .copied()
        .unwrap_or_else(|| mix(a, pal.accent2, 0.55));
    let dark = pal.card;
    let clip = painter.with_clip_rect(rect);
    clip.rect_filled(rect, radius, dark);
    gradient(
        &clip,
        rect,
        mix(a, dark, 0.15),
        mix(b, dark, 0.45),
        mix(a, dark, 0.55),
        mix(b, dark, 0.8),
    );
    // A soft glow circle gives the placeholder some depth.
    clip.circle_filled(
        rect.left_top() + vec2(rect.width() * 0.78, rect.height() * 0.3),
        rect.height() * 0.55,
        Color32::from_white_alpha(10),
    );
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(String::new, |c| c.to_uppercase().collect());
    clip.text(
        pos2(rect.center().x, rect.top() + rect.height() * 0.32),
        Align2::CENTER_CENTER,
        initial,
        FontId::proportional((rect.height() * 0.42).clamp(18.0, 72.0)),
        Color32::from_white_alpha(70),
    );
}

/// Draws a waveform from per-column (min, max) in `rect`. Columns before `played` (0..1)
/// draw in `hi`, the rest in `lo`.
pub fn waveform(
    painter: &egui::Painter,
    rect: Rect,
    cols: usize,
    get: impl Fn(usize) -> (f32, f32),
    lo: Color32,
    hi: Color32,
    played: Option<f32>,
) {
    if cols == 0 {
        return;
    }
    let mut m = Mesh::default();
    let w = rect.width() / cols as f32;
    let mid = rect.center().y;
    let half = rect.height() / 2.0;
    let cut = played.map_or(-1.0, |p| p * cols as f32);
    for c in 0..cols {
        let (mn, mx) = get(c);
        let x0 = rect.left() + c as f32 * w;
        let x1 = x0 + (w * 0.8).max(1.0);
        let y0 = mid - mx.clamp(-1.0, 1.0) * half;
        let y1 = (mid - mn.clamp(-1.0, 1.0) * half).max(y0 + 1.0);
        let color = if (c as f32) < cut { hi } else { lo };
        m.add_colored_rect(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), color);
    }
    painter.add(m);
}

/// A small rounded label; `fill` sets its colour.
pub fn chip(ui: &mut Ui, text: &str, fill: Color32, fg: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(11.5), fg);
    let size = galley.size() + vec2(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() {
        mix(fill, Color32::WHITE, 0.12)
    } else {
        fill
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// A genre chip in its colour; `on` draws it solid, otherwise outlined.
pub fn genre_chip(ui: &mut Ui, name: &str, color: Color32, on: bool) -> Response {
    let fg = if on {
        Color32::from_rgb(0x10, 0x12, 0x16)
    } else {
        color
    };
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_string(), FontId::proportional(11.5), fg);
    let size = galley.size() + vec2(16.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CornerRadius::same(255), color);
    } else {
        p.rect_filled(
            rect,
            CornerRadius::same(255),
            color.gamma_multiply(if resp.hovered() { 0.22 } else { 0.12 }),
        );
        p.rect_stroke(
            rect,
            CornerRadius::same(255),
            Stroke::new(1.0, color.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
    }
    p.galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// Five stars; returns the new rating when clicked (clicking the current rating clears it).
pub fn stars(ui: &mut Ui, rating: u8, pal: &Palette, size: f32) -> Option<u8> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for i in 1..=5u8 {
            let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
            let on = i <= rating;
            let hover = resp.hovered();
            let color = if on {
                pal.star
            } else if hover {
                pal.star.gamma_multiply(0.6)
            } else {
                pal.text_faint
            };
            star(ui.painter(), rect.center(), size * 0.45, color);
            if resp.clicked() {
                out = Some(if rating == i { 0 } else { i });
            }
        }
    });
    out
}

/// A filled five-point star.
pub fn star(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let mut m = Mesh::default();
    m.vertices.push(Vertex {
        pos: c,
        uv: WHITE_UV,
        color,
    });
    for k in 0..10 {
        let ang = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
        let rr = if k % 2 == 0 { r } else { r * 0.45 };
        m.vertices.push(Vertex {
            pos: c + vec2(ang.cos(), ang.sin()) * rr,
            uv: WHITE_UV,
            color,
        });
    }
    for k in 0..10u32 {
        m.indices.extend_from_slice(&[0, 1 + k, 1 + (k + 1) % 10]);
    }
    painter.add(m);
}

/// A heart-ish favourite toggle drawn as a star in a circle; returns true when clicked.
pub fn fav_button(ui: &mut Ui, rect: Rect, on: bool, pal: &Palette, id: egui::Id) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() {
        Color32::from_black_alpha(170)
    } else {
        Color32::from_black_alpha(110)
    };
    p.circle_filled(rect.center(), rect.width() / 2.0, bg);
    star(
        p,
        rect.center(),
        rect.width() * 0.3,
        if on {
            pal.star
        } else {
            Color32::from_white_alpha(150)
        },
    );
    resp.on_hover_text(if on { "Unfavorite" } else { "Favorite" })
        .clicked()
}

/// A section heading.
pub fn heading(ui: &mut Ui, text: &str, pal: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(pal.text_dim),
    );
}

/// Text cut to `max` characters with an ellipsis.
#[must_use]
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
