//! Where the library lives and how it's saved.
//!
//! The library is a data file in the user's data folder, separate from the code:
//! `%APPDATA%\SolPlugs\Samples\library.json` on Windows. Edited versions and artwork
//! thumbnails sit beside it in `edits\` and `art\`. A build or reinstall never touches it.

use std::path::{Path, PathBuf};

use crate::model::{LIBRARY_VERSION, Library};

/// The library's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub dir: PathBuf,
}

/// Why loading or saving failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("can't read or write the library: {0}")]
    Io(#[from] std::io::Error),
    #[error("the library file is damaged: {0}")]
    Damaged(String),
    #[error("the library was saved by a newer SolPlugs Samples (version {0})")]
    Newer(u32),
}

impl Store {
    /// The default location for this user.
    #[must_use]
    pub fn default_location() -> Self {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        }
        .unwrap_or_else(std::env::temp_dir);
        Self {
            dir: base.join("SolPlugs").join("Samples"),
        }
    }

    /// A store in `dir`.
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The index file.
    #[must_use]
    pub fn index(&self) -> PathBuf {
        self.dir.join("library.json")
    }

    /// Where edited versions are written.
    #[must_use]
    pub fn edits(&self) -> PathBuf {
        self.dir.join("edits")
    }

    /// Where artwork thumbnails and pack covers are kept.
    #[must_use]
    pub fn art(&self) -> PathBuf {
        self.dir.join("art")
    }

    /// Loads the library; a missing file is an empty library.
    pub fn load(&self) -> Result<Library, StoreError> {
        let path = self.index();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Library::default()),
            Err(e) => return Err(e.into()),
        };
        let lib: Library =
            serde_json::from_str(&text).map_err(|e| StoreError::Damaged(e.to_string()))?;
        if lib.version > LIBRARY_VERSION {
            return Err(StoreError::Newer(lib.version));
        }
        Ok(lib)
    }

    /// Saves atomically: write a temporary file, keep the previous one as `.bak`, rename.
    pub fn save(&self, lib: &Library) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.index();
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string(lib).map_err(|e| StoreError::Damaged(e.to_string()))?;
        std::fs::write(&tmp, text)?;
        if path.exists() {
            let _ = std::fs::copy(&path, path.with_extension("json.bak"));
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Modification time of the index file, to notice another instance's save.
    #[must_use]
    pub fn modified(&self) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.index())
            .and_then(|m| m.modified())
            .ok()
    }

    /// True if `path` is inside this store (an edit or artwork we own).
    #[must_use]
    pub fn owns(&self, path: &Path) -> bool {
        path.starts_with(&self.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let store = Store::at(dir.path());
        assert_eq!(store.load()?, Library::default());
        let mut lib = Library::default();
        let id = lib.new_id();
        lib.packs.push(crate::model::Pack {
            id,
            name: "Mine".into(),
            ..Default::default()
        });
        store.save(&lib)?;
        store.save(&lib)?;
        assert_eq!(store.load()?, lib);
        assert!(store.index().with_extension("json.bak").exists());
        Ok(())
    }

    #[test]
    fn damaged_file_is_an_error_not_a_wipe() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let store = Store::at(dir.path());
        std::fs::write(store.index(), "{ not json")?;
        assert!(matches!(store.load(), Err(StoreError::Damaged(_))));
        Ok(())
    }
}
