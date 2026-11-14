//! Genre colours: each genre gets its own colour so a pack reads at a glance.

use crate::model::Genre;

const DEFAULTS: &[(&str, [u8; 3])] = &[
    ("Hip Hop", [0xFF, 0x8A, 0x3D]),
    ("Trap", [0xFF, 0x4F, 0x7B]),
    ("House", [0x3D, 0xD6, 0xF5]),
    ("Techno", [0x9B, 0x7B, 0xFF]),
    ("Drum & Bass", [0x6B, 0xD3, 0x8A]),
    ("Dubstep", [0x2E, 0xD3, 0xC0]),
    ("Pop", [0xFF, 0x6F, 0xD8]),
    ("R&B", [0xC4, 0x7B, 0xFF]),
    ("Lo-Fi", [0xD9, 0xB3, 0x8C]),
    ("Ambient", [0x7F, 0xB8, 0xFF]),
    ("Rock", [0xE0, 0x4E, 0x3C]),
    ("Jazz", [0xFF, 0xC8, 0x57]),
    ("Afrobeats", [0xF2, 0xD0, 0x3B]),
    ("Cinematic", [0x8E, 0x9C, 0xB8]),
    ("Funk", [0xFF, 0xA5, 0x00]),
    ("Reggaeton", [0x4C, 0xE0, 0x6E]),
];

/// The built-in genres.
#[must_use]
pub fn defaults() -> Vec<Genre> {
    DEFAULTS
        .iter()
        .map(|(n, c)| Genre {
            name: (*n).to_string(),
            color: *c,
        })
        .collect()
}

/// A stable, bright colour for a genre the list doesn't know (hue from the name's hash).
#[must_use]
pub fn fallback_color(name: &str) -> [u8; 3] {
    let mut h: u32 = 2_166_136_261;
    for b in name.to_ascii_lowercase().bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16_777_619);
    }
    hsv(f64::from(h % 360), 0.62, 0.95)
}

/// HSV (degrees, 0..1, 0..1) to sRGB.
#[must_use]
pub fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_stable_and_case_blind() {
        assert_eq!(fallback_color("Phonk"), fallback_color("phonk"));
        assert_ne!(fallback_color("Phonk"), fallback_color("Grime"));
    }
}
