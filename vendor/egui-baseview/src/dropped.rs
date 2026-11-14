//! SolPlugs patch: a file dropped on the window, as egui expects it (see VENDORED.md).

use std::path::{Path, PathBuf};

/// A dropped file known by its path.
#[derive(Debug)]
pub(crate) struct PathFile(pub(crate) PathBuf);

impl egui::DroppedFile for PathFile {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}
