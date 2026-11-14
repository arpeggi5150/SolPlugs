//! Dragging samples out of the manager as files: onto a Solara track, into the Sampler, or
//! into any DAW. On Windows this is an OLE drag with the file list (what Explorer does), so
//! anything that accepts dropped files accepts a sample.
//!
//! Inside an egui host (Solara) the drag also carries [`SampleDrag`] as an egui payload, so
//! Solara can take it without going through the OS.

use std::path::PathBuf;

use raw_window_handle::HasWindowHandle;

/// The egui drag payload for samples dragged inside the same window.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleDrag {
    pub ids: Vec<String>,
    pub paths: Vec<PathBuf>,
}

/// True where an OS drag is available.
#[must_use]
pub fn supported() -> bool {
    cfg!(windows)
}

/// Starts an OS file drag from `window`. Blocks until the drop (the OS runs the drag loop).
/// Returns true if the files were dropped somewhere.
pub fn start(window: &dyn HasWindowHandle, paths: &[PathBuf], image: Option<PathBuf>) -> Result<bool, String> {
    imp::start(window, paths, image)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub fn start(window: &dyn HasWindowHandle, paths: &[PathBuf], image: Option<PathBuf>) -> Result<bool, String> {
        let paths: Vec<PathBuf> = paths.iter().filter(|p| p.is_file()).cloned().collect();
        if paths.is_empty() {
            return Err("the sample file can't be found".into());
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let d = dropped.clone();
        let img = image.filter(|p| p.is_file()).map_or(drag::Image::Raw(Vec::new()), drag::Image::File);
        // The drag crate unwraps internally when building the data object; keep any panic here.
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drag::start_drag(
                &window,
                drag::DragItem::Files(paths),
                img,
                move |r, _| d.store(matches!(r, drag::DragResult::Dropped), Ordering::Release),
                drag::Options::default(),
            )
        }));
        match res {
            Ok(Ok(())) => Ok(dropped.load(Ordering::Acquire)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("the drag failed".into()),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn start(_window: &dyn HasWindowHandle, _paths: &[PathBuf], _image: Option<PathBuf>) -> Result<bool, String> {
        Err("dragging files out isn't available on this platform yet".into())
    }
}
