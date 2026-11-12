//! The reference library: up to [`MAX_REFERENCES`] saved references, and groups of them.
//!
//! Only analysis is stored. A file reference also remembers its path so it can be played
//! again; a Deezer reference remembers its track id so its preview can be streamed again;
//! a capture keeps nothing but its analysis.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use solplugs_ref_core::profile::Profile;

/// How many references the library holds.
pub const MAX_REFERENCES: usize = 60;
/// Current library file version.
pub const LIBRARY_VERSION: u32 = 1;
/// Longest name kept for titles, artists and groups.
const MAX_TEXT: usize = 200;

/// Where a reference came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// An audio file on this machine.
    File { path: PathBuf },
    /// A 30-second Deezer preview (streamed, never stored).
    Deezer { track_id: u64, link: String },
    /// Audio captured from another app through SolRoute.
    Capture,
}

/// One saved reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub origin: Origin,
    pub profile: Profile,
    #[serde(default)]
    pub added_unix: u64,
}

/// A named set of references compared together.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub members: Vec<String>,
}

/// User settings stored with the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Online search (Deezer previews). Off until the user turns it on.
    #[serde(default)]
    pub online_search: bool,
    /// Tilt applied to tonal-balance curves for display, dB per octave.
    #[serde(default = "default_tilt")]
    pub display_tilt_db_per_oct: f64,
}

fn default_tilt() -> f64 {
    3.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            online_search: false,
            display_tilt_db_per_oct: default_tilt(),
        }
    }
}

/// The library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    pub references: Vec<Reference>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub settings: Settings,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: LIBRARY_VERSION,
            references: Vec::new(),
            groups: Vec::new(),
            settings: Settings::default(),
        }
    }
}

/// Why a library operation failed.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("the library is full ({MAX_REFERENCES} references); remove one first")]
    Full,
    #[error("can't read or write the library: {0}")]
    Io(#[from] std::io::Error),
    #[error("the library file is damaged: {0}")]
    Format(#[from] serde_json::Error),
}

fn clip(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

/// A stable id for a reference, from where it came from.
#[must_use]
pub fn id_for(origin: &Origin, title: &str) -> String {
    // FNV-1a: stable across runs and platforms, no dependency.
    let key = match origin {
        Origin::File { path } => format!("file:{}", path.to_string_lossy()),
        Origin::Deezer { track_id, .. } => format!("deezer:{track_id}"),
        Origin::Capture => format!("capture:{title}:{}", now_unix()),
    };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Library {
    /// Adds (or replaces, by id) a reference. Fails when full.
    pub fn add(
        &mut self,
        title: &str,
        artist: &str,
        origin: Origin,
        profile: Profile,
    ) -> Result<String, LibraryError> {
        let id = id_for(&origin, title);
        let entry = Reference {
            id: id.clone(),
            title: clip(title),
            artist: clip(artist),
            origin,
            profile,
            added_unix: now_unix(),
        };
        if let Some(existing) = self.references.iter_mut().find(|r| r.id == id) {
            *existing = entry;
        } else if self.references.len() >= MAX_REFERENCES {
            return Err(LibraryError::Full);
        } else {
            self.references.push(entry);
        }
        Ok(id)
    }

    /// Removes a reference and drops it from every group.
    pub fn remove(&mut self, id: &str) {
        self.references.retain(|r| r.id != id);
        for g in &mut self.groups {
            g.members.retain(|m| m != id);
        }
    }

    /// Finds a reference.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Reference> {
        self.references.iter().find(|r| r.id == id)
    }

    /// The group with this name, created if missing.
    pub fn group_mut(&mut self, name: &str) -> &mut Group {
        let name = clip(name);
        let idx = match self.groups.iter().position(|g| g.name == name) {
            Some(i) => i,
            None => {
                self.groups.push(Group {
                    name,
                    members: Vec::new(),
                });
                self.groups.len() - 1
            }
        };
        &mut self.groups[idx]
    }

    /// Renames a reference.
    pub fn rename(&mut self, id: &str, title: &str, artist: &str) {
        if let Some(r) = self.references.iter_mut().find(|r| r.id == id) {
            r.title = clip(title);
            r.artist = clip(artist);
        }
    }

    /// Makes a library read from disk safe to use: caps, fixed curve lengths, no dangling ids.
    pub fn normalise(&mut self) {
        self.version = LIBRARY_VERSION;
        let mut seen = std::collections::HashSet::new();
        self.references.retain(|r| seen.insert(r.id.clone()));
        self.references.truncate(MAX_REFERENCES);
        for r in &mut self.references {
            r.title = clip(&r.title);
            r.artist = clip(&r.artist);
            r.profile.normalise();
        }
        let ids: std::collections::HashSet<String> =
            self.references.iter().map(|r| r.id.clone()).collect();
        self.groups.truncate(64);
        for g in &mut self.groups {
            g.name = clip(&g.name);
            g.members.retain(|m| ids.contains(m));
            g.members.dedup();
        }
        if !self.settings.display_tilt_db_per_oct.is_finite() {
            self.settings.display_tilt_db_per_oct = default_tilt();
        }
        self.settings.display_tilt_db_per_oct =
            self.settings.display_tilt_db_per_oct.clamp(0.0, 6.0);
    }

    /// Merges another library (an import) into this one, up to the cap. Returns how many were added.
    pub fn merge(&mut self, other: Library) -> usize {
        let mut added = 0;
        for r in other.references {
            if self.get(&r.id).is_none() && self.references.len() < MAX_REFERENCES {
                self.references.push(r);
                added += 1;
            }
        }
        for g in other.groups {
            let mine = self.group_mut(&g.name);
            for m in g.members {
                if !mine.members.contains(&m) {
                    mine.members.push(m);
                }
            }
        }
        self.normalise();
        added
    }

    /// Reads a library file. A missing file is an empty library.
    pub fn load(path: &Path) -> Result<Self, LibraryError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let mut lib: Library = serde_json::from_slice(&bytes)?;
                lib.normalise();
                Ok(lib)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes the library atomically (temp file, then rename).
    pub fn save(&self, path: &Path) -> Result<(), LibraryError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// The default library location: `%APPDATA%\SolPlugs\Reference\library.json` on Windows,
/// `~/Library/Application Support/SolPlugs/Reference` on macOS, `$XDG_CONFIG_HOME/solplugs/reference` elsewhere.
#[must_use]
pub fn default_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let dir = if cfg!(any(windows, target_os = "macos")) {
        "SolPlugs/Reference"
    } else {
        "solplugs/reference"
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join(dir)
        .join("library.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_at_sixty_and_replaces_by_id() {
        let mut lib = Library::default();
        for i in 0..MAX_REFERENCES {
            let path = PathBuf::from(format!("/music/{i}.wav"));
            assert!(
                lib.add(
                    &format!("Song {i}"),
                    "Artist",
                    Origin::File { path },
                    Profile::default()
                )
                .is_ok()
            );
        }
        let extra = Origin::File {
            path: PathBuf::from("/music/extra.wav"),
        };
        assert!(matches!(
            lib.add("x", "y", extra, Profile::default()),
            Err(LibraryError::Full)
        ));
        // Same origin replaces instead of failing.
        let again = Origin::File {
            path: PathBuf::from("/music/3.wav"),
        };
        assert!(lib.add("Renamed", "A", again, Profile::default()).is_ok());
        assert_eq!(lib.references.len(), MAX_REFERENCES);
        assert!(lib.references.iter().any(|r| r.title == "Renamed"));
    }

    #[test]
    fn save_load_round_trip_and_groups_follow_removals() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("solplugs-lib-test-{}", std::process::id()));
        let path = dir.join("library.json");
        let mut lib = Library::default();
        let a = lib.add(
            "A",
            "x",
            Origin::Deezer {
                track_id: 1,
                link: String::new(),
            },
            Profile::default(),
        )?;
        let b = lib.add("B", "x", Origin::Capture, Profile::default())?;
        lib.group_mut("Club").members.extend([a.clone(), b.clone()]);
        lib.remove(&b);
        lib.save(&path)?;
        let back = Library::load(&path)?;
        assert_eq!(back.groups[0].members, vec![a]);
        assert!(
            !back.settings.online_search,
            "online search must default to off"
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn damaged_files_are_errors_and_missing_is_empty() {
        let missing = Library::load(Path::new("/definitely/not/here/library.json"));
        assert!(missing.is_ok_and(|l| l.references.is_empty()));
    }
}
