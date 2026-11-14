//! Pack export: a folder holding copies of the pack's samples (raws and edits), its cover,
//! and `pack.json` with all the metadata and the edit-to-raw links.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Library, Version};

/// Export file format version.
pub const EXPORT_VERSION: u32 = 1;

#[derive(Serialize)]
struct ExportSample {
    /// Path inside the export folder.
    file: String,
    name: String,
    /// "raw" or "edited".
    version: &'static str,
    /// For edits: the exported file of the sample it was made from, when that's in the pack too.
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_file: Option<String>,
    /// For edits: the raw it came from, by name (always present for edits).
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit: Option<String>,
    bpm: Option<f64>,
    key: Option<String>,
    duration_sec: f64,
    sample_rate: u32,
    bit_depth: Option<u8>,
    channels: u8,
    tags: Vec<String>,
    genres: Vec<String>,
    rating: u8,
    favorite: bool,
}

#[derive(Serialize)]
struct ExportPack {
    format: &'static str,
    version: u32,
    name: String,
    description: String,
    tags: Vec<String>,
    genres: Vec<String>,
    cover: Option<String>,
    samples: Vec<ExportSample>,
}

/// Why export failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("that pack isn't in the library")]
    NoPack,
    #[error("can't write the export: {0}")]
    Io(#[from] std::io::Error),
}

fn safe(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')' | ',') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "untitled".into() } else { t }
}

fn unique(dir: &Path, stem: &str, ext: &str, used: &mut HashSet<String>) -> String {
    let mut name = format!("{stem}.{ext}");
    let mut n = 2;
    while used.contains(&name.to_lowercase()) || dir.join(&name).exists() {
        name = format!("{stem} ({n}).{ext}");
        n += 1;
    }
    used.insert(name.to_lowercase());
    name
}

/// What an export produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub folder: PathBuf,
    pub copied: usize,
    /// Members whose files couldn't be found or copied.
    pub skipped: Vec<String>,
}

/// Exports pack `pack_id` into a new folder under `dest`.
pub fn export(lib: &Library, pack_id: &str, dest: &Path) -> Result<Exported, ExportError> {
    let pack = lib.pack(pack_id).ok_or(ExportError::NoPack)?;
    let mut folder = dest.join(safe(&pack.name));
    let mut n = 2;
    while folder.exists() {
        folder = dest.join(format!("{} ({n})", safe(&pack.name)));
        n += 1;
    }
    let raw_dir = folder.join("Raw");
    let edit_dir = folder.join("Edited");
    std::fs::create_dir_all(&raw_dir)?;
    let mut used = HashSet::new();
    let mut files: std::collections::HashMap<String, String> = Default::default();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    // Raws first so edits can point at them.
    let mut members: Vec<_> = pack
        .members
        .iter()
        .filter_map(|id| lib.sample(id))
        .collect();
    members.sort_by_key(|s| s.version.is_edited());
    for s in members {
        let (sub, dir) = if s.version.is_edited() {
            std::fs::create_dir_all(&edit_dir)?;
            ("Edited", &edit_dir)
        } else {
            ("Raw", &raw_dir)
        };
        let ext = s
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("wav")
            .to_ascii_lowercase();
        let name = unique(dir, &safe(&s.name), &ext, &mut used);
        if std::fs::copy(&s.path, dir.join(&name)).is_err() {
            skipped.push(s.name.clone());
            continue;
        }
        let file = format!("{sub}/{name}");
        files.insert(s.id.clone(), file.clone());
        let (parent_file, raw_name, edit) = match &s.version {
            Version::Raw => (None, None, None),
            Version::Edited { parent, root, edit } => (
                files.get(parent).cloned(),
                Some(
                    lib.sample(root)
                        .map_or_else(|| "(removed)".to_string(), |r| r.name.clone()),
                ),
                Some(edit.clone()),
            ),
        };
        out.push(ExportSample {
            file,
            name: s.name.clone(),
            version: if s.version.is_edited() {
                "edited"
            } else {
                "raw"
            },
            parent_file,
            raw_name,
            edit,
            bpm: s.bpm,
            key: s.key.clone(),
            duration_sec: s.duration_sec,
            sample_rate: s.sample_rate,
            bit_depth: s.bit_depth,
            channels: s.channels,
            tags: s.tags.clone(),
            genres: s.genres.clone(),
            rating: s.rating,
            favorite: s.favorite,
        });
    }
    let cover = pack.cover.as_ref().and_then(|c| {
        let ext = c.extension().and_then(|e| e.to_str()).unwrap_or("png");
        let name = format!("cover.{ext}");
        std::fs::copy(c, folder.join(&name)).ok().map(|_| name)
    });
    let doc = ExportPack {
        format: "solplugs-samples-pack",
        version: EXPORT_VERSION,
        name: pack.name.clone(),
        description: pack.description.clone(),
        tags: pack.tags.clone(),
        genres: pack.genres.clone(),
        cover,
        samples: out,
    };
    let json = serde_json::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    std::fs::write(folder.join("pack.json"), json)?;
    Ok(Exported {
        folder,
        copied: files.len(),
        skipped,
    })
}
