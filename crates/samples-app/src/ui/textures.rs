//! Artwork textures, loaded on demand (a few per frame so scrolling stays smooth).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{ColorImage, TextureHandle, TextureOptions};

/// Images loaded per frame at most.
const PER_FRAME: usize = 6;

/// Cached textures by file path.
#[derive(Default)]
pub struct Textures {
    map: HashMap<PathBuf, Option<TextureHandle>>,
    loaded_this_frame: usize,
}

impl Textures {
    /// Call once per frame.
    pub fn begin_frame(&mut self) {
        self.loaded_this_frame = 0;
    }

    /// The texture for `path`; `None` while not loaded yet or if it can't be read.
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        if let Some(t) = self.map.get(path) {
            return t.clone();
        }
        if self.loaded_this_frame >= PER_FRAME {
            ctx.request_repaint();
            return None;
        }
        self.loaded_this_frame += 1;
        let tex = solplugs_samples_core::art::load_rgba(path).map(|(w, h, px)| {
            let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            ctx.load_texture(path.to_string_lossy(), img, TextureOptions::LINEAR)
        });
        self.map.insert(path.to_path_buf(), tex.clone());
        tex
    }

    /// Forget a path (its image changed).
    pub fn forget(&mut self, path: &Path) {
        self.map.remove(path);
    }
}
