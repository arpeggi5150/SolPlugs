//! The engine: the library (locked for the UI), the preview player, the decoded-audio cache,
//! and the background threads that import, edit, export and save.
//!
//! One engine per process is shared by every plugin instance and the standalone window
//! ([`Engine::shared`]), so two instances never fight over the library file.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Instant;

use solplugs_samples_core::audio::{self, Audio};
use solplugs_samples_core::fx::Fx;
use solplugs_samples_core::model::Library;
use solplugs_samples_core::store::Store;

use crate::player::{Clip, Player};
use crate::worker::{self, Cmd};

/// Decoded audio kept for instant replays, in samples (all channels) - about 200 MB.
const CACHE_SAMPLES: usize = 50_000_000;

/// A message for the status bar.
#[derive(Clone, Debug)]
pub struct Note {
    pub at: Instant,
    pub text: String,
    pub error: bool,
}

/// Import progress.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

/// What the UI reads and edits, under one lock.
pub struct Shared {
    pub lib: Library,
    /// Bumped on every change, so the UI can cache filter results.
    pub revision: u64,
    pub dirty: bool,
    pub changed_at: Option<Instant>,
    pub import: Option<Progress>,
    pub notes: VecDeque<Note>,
    /// The library couldn't be loaded; it's kept read-only so nothing overwrites it.
    pub load_error: Option<String>,
}

impl Shared {
    /// Marks the library changed (saved shortly after).
    pub fn touch(&mut self) {
        self.revision += 1;
        self.dirty = true;
        self.changed_at = Some(Instant::now());
    }

    /// Adds a status message.
    pub fn note(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        }
        self.notes.push_back(Note {
            at: Instant::now(),
            text,
            error,
        });
        while self.notes.len() > 20 {
            self.notes.pop_front();
        }
    }
}

struct CacheEntry {
    audio: Arc<Audio>,
    used: Instant,
}

/// Decoded audio by sample id.
#[derive(Default)]
pub(crate) struct Cache {
    map: Mutex<HashMap<String, CacheEntry>>,
}

impl Cache {
    pub(crate) fn get(&self, id: &str) -> Option<Arc<Audio>> {
        let mut m = self.map.lock().ok()?;
        let e = m.get_mut(id)?;
        e.used = Instant::now();
        Some(e.audio.clone())
    }

    pub(crate) fn put(&self, id: &str, audio: Arc<Audio>) {
        let Ok(mut m) = self.map.lock() else { return };
        m.insert(id.to_string(), CacheEntry { audio, used: Instant::now() });
        let size = |m: &HashMap<String, CacheEntry>| {
            m.values().map(|e| e.audio.frames() * e.audio.channels.len()).sum::<usize>()
        };
        while m.len() > 1 && size(&m) > CACHE_SAMPLES {
            let Some(oldest) = m.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone()) else {
                break;
            };
            m.remove(&oldest);
        }
    }

    pub(crate) fn forget(&self, id: &str) {
        if let Ok(mut m) = self.map.lock() {
            m.remove(id);
        }
    }
}

/// A preview request.
pub(crate) enum Load {
    /// Decode into the cache (hover prefetch, detail view).
    Fetch { id: String, path: PathBuf },
    /// Decode if needed, render through `fx`, and play `from..to` seconds.
    Play { id: String, path: PathBuf, fx: Fx, from: f64, to: Option<f64> },
    /// Re-render the playing clip after a processing change, keeping the playhead.
    Refresh { id: String, path: PathBuf, fx: Fx },
}

/// One library engine.
pub struct Engine {
    store: Store,
    shared: Arc<Mutex<Shared>>,
    player: Arc<Player>,
    cache: Arc<Cache>,
    tx: Sender<Cmd>,
    load_tx: Sender<Load>,
    pub(crate) cancel: Arc<AtomicBool>,
}

static SHARED: OnceLock<Mutex<Weak<Engine>>> = OnceLock::new();

impl Engine {
    /// The process-wide engine on the default library, created on first use.
    #[must_use]
    pub fn shared() -> Arc<Self> {
        let slot = SHARED.get_or_init(|| Mutex::new(Weak::new()));
        let Ok(mut weak) = slot.lock() else {
            return Arc::new(Self::new(Store::default_location()));
        };
        if let Some(e) = weak.upgrade() {
            return e;
        }
        let e = Arc::new(Self::new(Store::default_location()));
        *weak = Arc::downgrade(&e);
        e
    }

    /// An engine on the library in `store`.
    #[must_use]
    pub fn new(store: Store) -> Self {
        let (lib, load_error) = match store.load() {
            Ok(l) => (l, None),
            Err(e) => (Library::default(), Some(e.to_string())),
        };
        let shared = Arc::new(Mutex::new(Shared {
            lib,
            revision: 1,
            dirty: false,
            changed_at: None,
            import: None,
            notes: VecDeque::new(),
            load_error,
        }));
        let player = Arc::new(Player::default());
        if let Ok(s) = shared.lock() {
            player.set_gain_db(s.lib.settings.preview_gain_db);
        }
        let cache = Arc::new(Cache::default());
        let (tx, rx) = channel();
        let (load_tx, load_rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let w = worker::Worker::new(store.clone(), shared.clone(), cache.clone(), cancel.clone());
        spawn("solplugs-samples", move || w.run(rx));
        let (p, c, s) = (player.clone(), cache.clone(), shared.clone());
        spawn("solplugs-samples-preview", move || preview_loop(&load_rx, &p, &c, &s));
        Self {
            store,
            shared,
            player,
            cache,
            tx,
            load_tx,
            cancel,
        }
    }

    /// The library's folder.
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Locks the shared state. Keep the guard short-lived (one UI frame at most).
    pub fn lock(&self) -> Option<MutexGuard<'_, Shared>> {
        self.shared.lock().ok()
    }

    /// The preview player, for the audio thread.
    #[must_use]
    pub fn player(&self) -> Arc<Player> {
        self.player.clone()
    }

    /// Asks the worker to do something.
    pub fn send(&self, cmd: Cmd) {
        if matches!(cmd, Cmd::CancelImport) {
            self.cancel.store(true, Ordering::Release);
            return;
        }
        let _ = self.tx.send(cmd);
    }

    /// Decoded audio of `id` if it's cached; otherwise starts decoding it and returns `None`.
    #[must_use]
    pub fn audio(&self, id: &str, path: &std::path::Path) -> Option<Arc<Audio>> {
        let a = self.cache.get(id);
        if a.is_none() {
            self.prefetch(id, path);
        }
        a
    }

    /// Starts decoding `id` so a click plays at once.
    pub fn prefetch(&self, id: &str, path: &std::path::Path) {
        let _ = self.load_tx.send(Load::Fetch { id: id.into(), path: path.into() });
    }

    /// Plays sample `id` (through its processing) from `from` to `to` seconds.
    pub fn preview(&self, id: &str, path: &std::path::Path, fx: &Fx, from: f64, to: Option<f64>) {
        let _ = self.load_tx.send(Load::Play {
            id: id.into(),
            path: path.into(),
            fx: fx.clone(),
            from,
            to,
        });
    }

    /// Re-renders the playing preview after its processing changed.
    pub fn refresh_preview(&self, id: &str, path: &std::path::Path, fx: &Fx) {
        if self.player.clip_id().as_deref() == Some(id) {
            let _ = self.load_tx.send(Load::Refresh { id: id.into(), path: path.into(), fx: fx.clone() });
        }
    }

    /// Stops the preview.
    pub fn stop(&self) {
        self.player.stop();
    }

    /// Forgets cached audio of `id` (its file changed).
    pub fn forget(&self, id: &str) {
        self.cache.forget(id);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.tx.send(Cmd::Quit);
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        log::error!("can't start {name}: {e}");
    }
}

fn load(cache: &Cache, id: &str, path: &std::path::Path) -> Result<Arc<Audio>, String> {
    if let Some(a) = cache.get(id) {
        return Ok(a);
    }
    let a = Arc::new(audio::decode_file(path).map_err(|e| e.to_string())?);
    cache.put(id, a.clone());
    Ok(a)
}

fn render(audio: &Arc<Audio>, fx: &Fx) -> Arc<Audio> {
    if fx.is_neutral() { audio.clone() } else { Arc::new(fx.render(audio)) }
}

fn preview_loop(rx: &Receiver<Load>, player: &Player, cache: &Cache, shared: &Mutex<Shared>) {
    while let Ok(first) = rx.recv() {
        // Coalesce: only the newest play request matters; fetches are cheap to keep.
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let last_play = batch.iter().rposition(|l| matches!(l, Load::Play { .. } | Load::Refresh { .. }));
        for (i, req) in batch.into_iter().enumerate() {
            let skip = matches!(req, Load::Play { .. } | Load::Refresh { .. }) && Some(i) != last_play;
            if skip {
                continue;
            }
            match req {
                Load::Fetch { id, path } => {
                    let _ = load(cache, &id, &path);
                }
                Load::Play { id, path, fx, from, to } => match load(cache, &id, &path) {
                    Ok(a) => player.play(Arc::new(Clip { id, audio: render(&a, &fx) }), from, to),
                    Err(e) => {
                        if let Ok(mut s) = shared.lock() {
                            s.note(format!("Can't play {}: {e}", path.display()), true);
                        }
                    }
                },
                Load::Refresh { id, path, fx } => {
                    if let Ok(a) = load(cache, &id, &path) {
                        player.replace(Arc::new(Clip { id, audio: render(&a, &fx) }));
                    }
                }
            }
        }
    }
}
