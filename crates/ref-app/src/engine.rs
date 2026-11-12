//! The engine: owns the background worker and the shared state, and is what a host embeds.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::bridge::Bridge;
use crate::state::{AppState, Command, SavedState};
use crate::worker::{Msg, Worker};

/// Saved-state format version.
const SAVED_VERSION: u32 = 1;

/// One analyzer instance (one per plugin instance, one in Solara's master bus).
pub struct Engine {
    bridge: Arc<Bridge>,
    state: Arc<Mutex<AppState>>,
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// An engine using the default library location.
    #[must_use]
    pub fn new() -> Self {
        Self::with_library(solplugs_ref_io::library::default_path())
    }

    /// An engine using the library file at `path`.
    #[must_use]
    pub fn with_library(path: PathBuf) -> Self {
        let bridge = Arc::new(Bridge::default());
        let state = Arc::new(Mutex::new(AppState {
            library_path: path,
            level_match: true,
            ..AppState::default()
        }));
        let (tx, rx) = channel();
        let worker = Worker::new(bridge.clone(), state.clone(), rx);
        let thread = std::thread::Builder::new()
            .name("solplugs-reference".into())
            .spawn(move || worker.run())
            .map_err(|e| log::error!("can't start the reference worker: {e}"))
            .ok();
        Self {
            bridge,
            state,
            tx,
            thread,
        }
    }

    /// The audio-thread bridge, for [`crate::processor::Processor::new`].
    #[must_use]
    pub fn bridge(&self) -> Arc<Bridge> {
        self.bridge.clone()
    }

    /// Runs `f` with the state locked. `None` if a thread panicked while holding it.
    pub fn with_state<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
        self.state.lock().ok().map(|mut s| f(&mut s))
    }

    /// Asks the worker to do something.
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }

    /// A/B: hear the reference (true) or the mix (false).
    pub fn set_listen_reference(&self, on: bool) {
        self.bridge.set_listen_reference(on);
    }

    /// What to store in the DAW project.
    #[must_use]
    pub fn save_state(&self) -> Vec<u8> {
        let saved = self
            .with_state(|s| SavedState {
                version: SAVED_VERSION,
                group: s.group.clone(),
                active: s.active.clone(),
                listening_reference: self.bridge.listening_to_reference(),
                level_match: s.level_match,
                view: s.view,
            })
            .unwrap_or_default();
        serde_json::to_vec(&saved).unwrap_or_default()
    }

    /// Restores what [`Self::save_state`] stored. Unknown or damaged data is ignored.
    pub fn load_state(&self, bytes: &[u8]) {
        if let Ok(saved) = serde_json::from_slice::<SavedState>(bytes) {
            self.bridge.set_listen_reference(saved.listening_reference);
            let _ = self.tx.send(Msg::Restore(saved));
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
