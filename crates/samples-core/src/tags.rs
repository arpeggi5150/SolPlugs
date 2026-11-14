//! Reads the tags and stream properties already in a file, through lofty (MIT OR Apache-2.0).

use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey};

/// What a file says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub comment: Option<String>,
    pub genres: Vec<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub duration_sec: Option<f64>,
    /// Embedded artwork (encoded image bytes), front cover preferred.
    pub picture: Option<Vec<u8>>,
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim().trim_matches('\0').trim();
    (!t.is_empty()).then(|| t.chars().take(200).collect())
}

/// Reads tags and properties. Never fails: an unreadable tag just gives empty fields.
#[must_use]
pub fn read(path: &Path) -> FileInfo {
    let mut info = FileInfo::default();
    // lofty parses untrusted bytes; keep a panic in it from reaching the host.
    let parsed = std::panic::catch_unwind(|| lofty::read_from_path(path));
    let Ok(Ok(file)) = parsed else {
        return info;
    };
    let p = file.properties();
    info.sample_rate = p.sample_rate();
    info.bit_depth = p.bit_depth();
    info.channels = p.channels();
    let d = p.duration().as_secs_f64();
    info.duration_sec = (d > 0.0).then_some(d);
    for tag in file.tags() {
        if info.title.is_none() {
            info.title = tag.title().as_deref().and_then(clean);
        }
        if info.artist.is_none() {
            info.artist = tag.artist().as_deref().and_then(clean);
        }
        if info.comment.is_none() {
            info.comment = tag.comment().as_deref().and_then(clean);
        }
        for g in tag.get_strings(&ItemKey::Genre) {
            // ID3 allows "Rock/Pop" and "(17)"-style lists; keep readable names only.
            for part in g.split(['/', ';', ',', '\0']) {
                if let Some(c) = clean(part)
                    && !c.starts_with('(')
                    && !info.genres.iter().any(|x| x.eq_ignore_ascii_case(&c))
                {
                    info.genres.push(c);
                }
            }
        }
        if info.bpm.is_none() {
            info.bpm = [ItemKey::Bpm, ItemKey::IntegerBpm]
                .iter()
                .filter_map(|k| tag.get_string(k))
                .find_map(|s| s.trim().replace(',', ".").parse::<f64>().ok())
                .filter(|v| (20.0..=400.0).contains(v));
        }
        if info.key.is_none() {
            info.key = tag
                .get_string(&ItemKey::InitialKey)
                .and_then(crate::keys::normalize);
        }
        if info.picture.is_none() {
            let pics = tag.pictures();
            let front = pics
                .iter()
                .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
                .or_else(|| pics.first());
            info.picture = front.map(|p| p.data().to_vec()).filter(|d| !d.is_empty());
        }
    }
    info
}
