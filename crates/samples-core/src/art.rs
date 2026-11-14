//! Artwork: thumbnails of embedded cover art, folder images and pack covers, kept in the
//! library's art folder as small PNGs so cards load fast.

use std::path::{Path, PathBuf};

/// Thumbnail edge, pixels.
pub const THUMB: u32 = 320;

/// Image files looked for beside samples, in order.
const FOLDER_IMAGES: &[&str] = &[
    "cover.png",
    "cover.jpg",
    "cover.jpeg",
    "folder.jpg",
    "folder.png",
    "artwork.png",
    "artwork.jpg",
    "front.jpg",
    "front.png",
];

/// Makes a square thumbnail from encoded image bytes and saves it as `<art>/<name>.png`.
pub fn save_thumb(bytes: &[u8], art_dir: &Path, name: &str) -> Option<PathBuf> {
    let img = std::panic::catch_unwind(|| image::load_from_memory(bytes))
        .ok()?
        .ok()?;
    let side = img.width().min(img.height());
    let x = (img.width() - side) / 2;
    let y = (img.height() - side) / 2;
    let thumb = img
        .crop_imm(x, y, side, side)
        .resize_exact(
            THUMB.min(side.max(1)),
            THUMB.min(side.max(1)),
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    std::fs::create_dir_all(art_dir).ok()?;
    let out = art_dir.join(format!("{name}.png"));
    thumb.save(&out).ok()?;
    Some(out)
}

/// A thumbnail of an image file.
pub fn thumb_from_file(path: &Path, art_dir: &Path, name: &str) -> Option<PathBuf> {
    let bytes = std::fs::read(path).ok()?;
    save_thumb(&bytes, art_dir, name)
}

/// A cover image in `folder` or its parent ("cover.jpg", "folder.png", ...), if any.
#[must_use]
pub fn folder_image(folder: &Path) -> Option<PathBuf> {
    for dir in [Some(folder), folder.parent()].into_iter().flatten() {
        for name in FOLDER_IMAGES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
            // Case-insensitive file systems handle "Cover.JPG"; on others try the capitalised name.
            let mut cap = name.to_string();
            cap[..1].make_ascii_uppercase();
            let p = dir.join(cap);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Loads a thumbnail as RGBA for drawing: (width, height, pixels).
#[must_use]
pub fn load_rgba(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let img = std::panic::catch_unwind(|| image::open(path)).ok()?.ok()?;
    let img = if img.width() > 1024 || img.height() > 1024 {
        img.thumbnail(1024, 1024)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}
