//! The library's data: samples (raw and edited versions), packs, genres and settings.
//!
//! The library is a data file (see [`crate::store`]), never part of the code, so rebuilding
//! or reinstalling the plugin can't touch it. Raw samples stay where they were imported from;
//! only their paths and metadata live here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fx::Fx;
use crate::peaks::Peaks;

/// Current library file version.
pub const LIBRARY_VERSION: u32 = 1;

/// A stable id (hex), unique within one library.
pub type Id = String;

/// Audio container of a sample file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Wav,
    Aiff,
    Flac,
    Mp3,
}

impl FileType {
    /// From a file extension (any case).
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "wav" | "wave" => Some(Self::Wav),
            "aif" | "aiff" | "aifc" => Some(Self::Aiff),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }

    /// Short label for cards.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
        }
    }
}

/// Where a BPM or key value came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Not known.
    #[default]
    None,
    /// Read from the file's own tags.
    Tag,
    /// Read from the file name ("..._120bpm_Amin.wav").
    Name,
    /// Detected by Song Sense on import.
    Detected,
    /// Typed in by the user.
    User,
}

/// Raw (imported as-is) or an edited version linked to the raw it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Version {
    /// The original file as imported, where it was found.
    Raw,
    /// The user's version, rendered into the library's edits folder.
    Edited {
        /// The sample this was made from (a raw or another edit).
        parent: Id,
        /// The raw at the root of the chain, so an edit of an edit still knows its origin.
        root: Id,
        /// What was done, in plain words ("Trim 0.25-1.50 s, Pre FX").
        edit: String,
    },
}

impl Version {
    /// True for an edited version.
    #[must_use]
    pub fn is_edited(&self) -> bool {
        matches!(self, Self::Edited { .. })
    }
}

/// One sample.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub id: Id,
    pub path: PathBuf,
    /// Display name (file stem unless renamed).
    pub name: String,
    pub version: Version,
    pub file_type: FileType,
    pub sample_rate: u32,
    #[serde(default)]
    pub bit_depth: Option<u8>,
    pub channels: u8,
    pub duration_sec: f64,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub bpm_source: Source,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_source: Source,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// 0 (unrated) to 5.
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub favorite: bool,
    pub added_unix: u64,
    /// Artist / comment from the file's tags, shown on the detail panel.
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub comment: String,
    /// Artwork thumbnail in the library's art folder (from the file or its folder).
    #[serde(default)]
    pub art: Option<PathBuf>,
    /// Waveform overview drawn on the card.
    #[serde(default)]
    pub peaks: Peaks,
    /// Processing set in the manager, used for previews and Post FX renders.
    #[serde(default)]
    pub fx: Fx,
    /// The file couldn't be found on the last check.
    #[serde(default)]
    pub missing: bool,
}

/// A pack: the user's own named group of samples (raws and edits).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Cover image in the library's art folder.
    #[serde(default)]
    pub cover: Option<PathBuf>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Sample ids, in the pack's order.
    #[serde(default)]
    pub members: Vec<Id>,
    pub created_unix: u64,
}

/// A genre and its colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Genre {
    pub name: String,
    /// sRGB.
    pub color: [u8; 3],
}

/// User settings stored with the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Card width in logical pixels.
    #[serde(default = "default_card")]
    pub card_width: f32,
    /// Show the live spectrum next to the waveform.
    #[serde(default = "yes")]
    pub spectrum: bool,
    /// Preview level, dB.
    #[serde(default)]
    pub preview_gain_db: f32,
    /// Folders imported so far, for rescans.
    #[serde(default)]
    pub folders: Vec<PathBuf>,
}

fn default_card() -> f32 {
    220.0
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            card_width: default_card(),
            spectrum: true,
            preview_gain_db: 0.0,
            folders: Vec::new(),
        }
    }
}

/// The whole library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    #[serde(default)]
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub packs: Vec<Pack>,
    #[serde(default = "crate::genres::defaults")]
    pub genres: Vec<Genre>,
    #[serde(default)]
    pub settings: Settings,
    /// Next id counter.
    #[serde(default)]
    pub next_id: u64,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: LIBRARY_VERSION,
            samples: Vec::new(),
            packs: Vec::new(),
            genres: crate::genres::defaults(),
            settings: Settings::default(),
            next_id: 1,
        }
    }
}

impl Library {
    /// A fresh id.
    pub fn new_id(&mut self) -> Id {
        let id = format!("{:08x}", self.next_id.max(1));
        self.next_id = self.next_id.max(1) + 1;
        id
    }

    /// The sample with `id`.
    #[must_use]
    pub fn sample(&self, id: &str) -> Option<&Sample> {
        self.samples.iter().find(|s| s.id == id)
    }

    /// The sample with `id`, mutable.
    pub fn sample_mut(&mut self, id: &str) -> Option<&mut Sample> {
        self.samples.iter_mut().find(|s| s.id == id)
    }

    /// The pack with `id`.
    #[must_use]
    pub fn pack(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }

    /// The pack with `id`, mutable.
    pub fn pack_mut(&mut self, id: &str) -> Option<&mut Pack> {
        self.packs.iter_mut().find(|p| p.id == id)
    }

    /// The sample already imported from `path`, if any.
    #[must_use]
    pub fn by_path(&self, path: &std::path::Path) -> Option<&Sample> {
        self.samples.iter().find(|s| s.path == path)
    }

    /// Edited versions made directly from `id`.
    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Sample> + 'a {
        self.samples
            .iter()
            .filter(move |s| matches!(&s.version, Version::Edited { parent, .. } if parent == id))
    }

    /// The raw at the root of `id`'s chain (itself for a raw).
    #[must_use]
    pub fn root_of<'a>(&'a self, id: &'a str) -> &'a str {
        match self.sample(id).map(|s| &s.version) {
            Some(Version::Edited { root, .. }) => root,
            _ => id,
        }
    }

    /// The colour of genre `name` (a stable fallback for genres not in the list).
    #[must_use]
    pub fn genre_color(&self, name: &str) -> [u8; 3] {
        self.genres
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(name))
            .map_or_else(|| crate::genres::fallback_color(name), |g| g.color)
    }

    /// Every free tag in use, sorted, with its count.
    #[must_use]
    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut m: std::collections::BTreeMap<String, usize> = Default::default();
        for s in &self.samples {
            for t in &s.tags {
                *m.entry(t.clone()).or_default() += 1;
            }
        }
        m.into_iter().collect()
    }

    /// Removes a sample, its links from packs, and re-parents its edits onto its parent.
    /// Returns the removed sample.
    pub fn remove_sample(&mut self, id: &str) -> Option<Sample> {
        let i = self.samples.iter().position(|s| s.id == id)?;
        let gone = self.samples.remove(i);
        for p in &mut self.packs {
            p.members.retain(|m| m != id);
        }
        // Children of a removed edit point at its parent; children of a removed raw keep
        // their `root` (so they still say where they came from) but lose the direct parent.
        let (new_parent, new_root) = match &gone.version {
            Version::Edited { parent, root, .. } => (parent.clone(), root.clone()),
            Version::Raw => (gone.id.clone(), gone.id.clone()),
        };
        for s in &mut self.samples {
            if let Version::Edited { parent, root, .. } = &mut s.version
                && *parent == gone.id
            {
                *parent = new_parent.clone();
                *root = new_root.clone();
            }
        }
        Some(gone)
    }
}

/// Seconds since the Unix epoch.
#[must_use]
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
