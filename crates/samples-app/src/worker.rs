//! The background worker: imports, edits, exports, artwork, and saving the library.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use solplugs_samples_core::edits::{self, Edit};
use solplugs_samples_core::model::{Id, Sample, Source};
use solplugs_samples_core::store::Store;
use solplugs_samples_core::{art, audio, export, scan};

use crate::engine::{Cache, Progress, Shared};

/// Something for the worker to do.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Import folders or files (samples stay where they are).
    Import(Vec<PathBuf>),
    /// Look for new files in every imported folder and flag missing ones.
    Rescan,
    /// Stop a running import.
    CancelImport,
    /// Save a tool result or a selection as edited version(s) of a sample.
    Edit { id: Id, edit: Edit },
    /// Export a pack into a new folder under `dest`.
    Export { pack: Id, dest: PathBuf },
    /// Use an image file as a pack's cover.
    SetCover { pack: Id, image: PathBuf },
    /// Use an image file as a sample's artwork.
    SetArt { sample: Id, image: PathBuf },
    /// Run Song Sense again on a sample (replacing detected, never user-typed, values).
    Detect { id: Id },
    /// Save now.
    Save,
    /// Stop the worker.
    Quit,
}

/// Wait after the last change before saving.
const SAVE_AFTER: Duration = Duration::from_millis(800);

pub(crate) struct Worker {
    store: Store,
    shared: Arc<Mutex<Shared>>,
    cache: Arc<Cache>,
    cancel: Arc<AtomicBool>,
    saved_mtime: Option<SystemTime>,
    last_check: Instant,
}

impl Worker {
    pub(crate) fn new(store: Store, shared: Arc<Mutex<Shared>>, cache: Arc<Cache>, cancel: Arc<AtomicBool>) -> Self {
        let saved_mtime = store.modified();
        Self {
            store,
            shared,
            cache,
            cancel,
            saved_mtime,
            last_check: Instant::now(),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Shared) -> R) -> Option<R> {
        self.shared.lock().ok().map(|mut s| f(&mut s))
    }

    pub(crate) fn run(mut self, rx: Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => {
                    self.save(true);
                    return;
                }
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.save(false);
            self.watch_file();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Import(paths) => self.import(&paths, true),
            Cmd::Rescan => self.rescan(),
            Cmd::Edit { id, edit } => self.edit(&id, edit),
            Cmd::Export { pack, dest } => self.export(&pack, &dest),
            Cmd::SetCover { pack, image } => self.set_cover(&pack, &image),
            Cmd::SetArt { sample, image } => self.set_art(&sample, &image),
            Cmd::Detect { id } => self.detect(&id),
            Cmd::Save => self.save(true),
            Cmd::CancelImport | Cmd::Quit => {}
        }
    }

    /// Saves when changed and quiet for a moment (or now when `force`).
    fn save(&mut self, force: bool) {
        let store = self.store.clone();
        let saved = self.with(|s| {
            if !s.dirty || s.load_error.is_some() {
                return None;
            }
            if !force && s.changed_at.is_some_and(|t| t.elapsed() < SAVE_AFTER) {
                return None;
            }
            match store.save(&s.lib) {
                Ok(()) => {
                    s.dirty = false;
                    Some(true)
                }
                Err(e) => {
                    s.note(format!("Couldn't save the library: {e}"), true);
                    s.changed_at = Some(Instant::now());
                    Some(false)
                }
            }
        });
        if saved.flatten() == Some(true) {
            self.saved_mtime = self.store.modified();
        }
    }

    /// Reloads when another process (the standalone window, another DAW) saved the library
    /// and we have nothing unsaved.
    fn watch_file(&mut self) {
        if self.last_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_check = Instant::now();
        let m = self.store.modified();
        if m.is_none() || m == self.saved_mtime {
            return;
        }
        let Ok(lib) = self.store.load() else { return };
        let reloaded = self.with(|s| {
            if s.dirty || s.import.is_some() {
                return false;
            }
            s.lib = lib;
            s.revision += 1;
            s.load_error = None;
            true
        });
        if reloaded == Some(true) {
            self.saved_mtime = m;
        }
    }

    fn import(&mut self, roots: &[PathBuf], remember: bool) {
        self.cancel.store(false, Ordering::Release);
        let mut paths = Vec::new();
        for r in roots {
            paths.extend(scan::collect(r));
        }
        let known: Vec<PathBuf> = self
            .with(|s| {
                if remember {
                    for r in roots.iter().filter(|r| r.is_dir()) {
                        if !s.lib.settings.folders.contains(r) {
                            s.lib.settings.folders.push(r.clone());
                        }
                    }
                }
                paths.iter().filter(|p| s.lib.by_path(p).is_some()).cloned().collect()
            })
            .unwrap_or_default();
        paths.retain(|p| !known.contains(p));
        let total = paths.len();
        self.with(|s| s.import = Some(Progress { done: 0, total, current: String::new() }));
        let genres: Vec<String> = self
            .with(|s| s.lib.genres.iter().map(|g| g.name.clone()).collect())
            .unwrap_or_default();
        let art_dir = self.store.art();
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let failed = Mutex::new(Vec::<(PathBuf, String)>::new());
        let added = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(1, 8));
        std::thread::scope(|sc| {
            for _ in 0..threads {
                sc.spawn(|| {
                    loop {
                        if self.cancel.load(Ordering::Acquire) {
                            return;
                        }
                        let i = next.fetch_add(1, Ordering::AcqRel);
                        let Some(p) = paths.get(i) else { return };
                        let probed = std::panic::catch_unwind(|| scan::probe(p, &genres, &art_dir))
                            .unwrap_or_else(|_| Err("this file crashed the reader".into()));
                        let d = done.fetch_add(1, Ordering::AcqRel) + 1;
                        self.with(|s| {
                            if let Some(pr) = s.import.as_mut() {
                                pr.done = d;
                                pr.current = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            }
                            match probed {
                                Ok(mut sample) => {
                                    if s.lib.by_path(p).is_none() {
                                        sample.id = s.lib.new_id();
                                        s.lib.samples.push(sample);
                                        added.fetch_add(1, Ordering::AcqRel);
                                        s.touch();
                                    }
                                }
                                Err(e) => {
                                    if let Ok(mut f) = failed.lock() {
                                        f.push((p.clone(), e));
                                    }
                                }
                            }
                        });
                    }
                });
            }
        });
        let failed = failed.into_inner().unwrap_or_default();
        let added = added.into_inner();
        let cancelled = self.cancel.swap(false, Ordering::AcqRel);
        self.with(|s| {
            s.import = None;
            let mut msg = format!("Imported {added} sample{}", if added == 1 { "" } else { "s" });
            if !known.is_empty() {
                msg.push_str(&format!(", {} already in the library", known.len()));
            }
            if cancelled {
                msg.push_str(" (stopped)");
            }
            s.note(msg, false);
            if !failed.is_empty() {
                let first = &failed[0];
                s.note(
                    format!(
                        "Skipped {} file{} that couldn't be read, e.g. {}: {}",
                        failed.len(),
                        if failed.len() == 1 { "" } else { "s" },
                        first.0.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        first.1
                    ),
                    true,
                );
            }
            s.touch();
        });
        self.save(true);
    }

    fn rescan(&mut self) {
        let folders = self
            .with(|s| {
                let mut missing = 0;
                for smp in &mut s.lib.samples {
                    let gone = !smp.path.is_file();
                    if gone != smp.missing {
                        smp.missing = gone;
                    }
                    missing += usize::from(gone);
                }
                if missing > 0 {
                    s.note(format!("{missing} sample file(s) can't be found"), true);
                }
                s.touch();
                s.lib.settings.folders.clone()
            })
            .unwrap_or_default();
        self.import(&folders, false);
    }

    fn load_audio(&self, sample: &Sample) -> Result<Arc<audio::Audio>, String> {
        if let Some(a) = self.cache.get(&sample.id) {
            return Ok(a);
        }
        let a = Arc::new(audio::decode_file(&sample.path).map_err(|e| e.to_string())?);
        self.cache.put(&sample.id, a.clone());
        Ok(a)
    }

    fn edit(&mut self, id: &str, edit: Edit) {
        let Some(sample) = self.with(|s| s.lib.sample(id).cloned()).flatten() else { return };
        let audio = match self.load_audio(&sample) {
            Ok(a) => a,
            Err(e) => {
                self.with(|s| s.note(format!("Can't edit {}: {e}", sample.name), true));
                return;
            }
        };
        let store = self.store.clone();
        self.with(|s| match edits::save(&mut s.lib, &store, id, &audio, edit) {
            Ok(ids) => {
                let names: Vec<String> = ids.iter().filter_map(|i| s.lib.sample(i)).map(|x| x.name.clone()).collect();
                s.note(format!("Saved {}", names.join(" and ")), false);
                s.touch();
            }
            Err(e) => s.note(format!("Couldn't save the edit: {e}"), true),
        });
        self.save(true);
    }

    fn export(&mut self, pack: &str, dest: &Path) {
        let Some(lib) = self.with(|s| s.lib.clone()) else { return };
        let res = export::export(&lib, pack, dest);
        self.with(|s| match res {
            Ok(out) => {
                let mut msg = format!("Exported {} sample(s) to {}", out.copied, out.folder.display());
                if !out.skipped.is_empty() {
                    msg.push_str(&format!(" ({} missing file(s) skipped)", out.skipped.len()));
                }
                s.note(msg, !out.skipped.is_empty());
            }
            Err(e) => s.note(format!("Export failed: {e}"), true),
        });
    }

    fn set_cover(&mut self, pack: &str, image: &Path) {
        let name = format!("pack-{pack}-{}", solplugs_samples_core::model::now_unix());
        let thumb = art::thumb_from_file(image, &self.store.art(), &name);
        self.with(|s| match (thumb, s.lib.pack_mut(pack)) {
            (Some(t), Some(p)) => {
                p.cover = Some(t);
                s.touch();
            }
            (None, _) => s.note(format!("Can't read {} as an image", image.display()), true),
            _ => {}
        });
    }

    fn set_art(&mut self, sample: &str, image: &Path) {
        let name = format!("smp-{sample}-{}", solplugs_samples_core::model::now_unix());
        let thumb = art::thumb_from_file(image, &self.store.art(), &name);
        self.with(|s| match (thumb, s.lib.sample_mut(sample)) {
            (Some(t), Some(x)) => {
                x.art = Some(t);
                s.touch();
            }
            (None, _) => s.note(format!("Can't read {} as an image", image.display()), true),
            _ => {}
        });
    }

    fn detect(&mut self, id: &str) {
        let Some(mut sample) = self.with(|s| s.lib.sample(id).cloned()).flatten() else { return };
        if !solplugs_songsense::available() {
            self.with(|s| s.note("Song Sense isn't in this build, so BPM/key can't be detected", true));
            return;
        }
        let Ok(audio) = self.load_audio(&sample) else { return };
        if sample.bpm_source == Source::Detected {
            sample.bpm = None;
        }
        if sample.key_source == Source::Detected {
            sample.key = None;
        }
        let before = (sample.bpm, sample.key.clone());
        scan::fill_detected(&mut sample, &audio);
        self.with(|s| {
            if let Some(x) = s.lib.sample_mut(id) {
                x.bpm = sample.bpm;
                x.bpm_source = sample.bpm_source;
                x.key = sample.key.clone();
                x.key_source = sample.key_source;
            }
            if (sample.bpm, sample.key.clone()) == before && before.0.is_none() {
                s.note(format!("Song Sense found no tempo in {}", sample.name), false);
            }
            s.touch();
        });
    }
}
