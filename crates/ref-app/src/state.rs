//! What the UI shows and edits. The worker keeps it current; the UI locks it briefly per frame.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use solplugs_ref_core::diff::Diff;
use solplugs_ref_core::group::GroupStats;
use solplugs_ref_core::live::LiveReadings;
use solplugs_ref_core::profile::Profile;
use solplugs_ref_io::library::{Library, Origin};

/// A reference the UI can list: from the library, or loaded this session only
/// (an unsaved preview or capture).
#[derive(Clone, Debug, PartialEq)]
pub struct RefEntry {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub origin: Origin,
    pub profile: Profile,
    pub in_library: bool,
}

/// One online search hit (kept free of the Deezer types so the UI builds without the feature).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u32,
    pub preview_url: String,
    pub link: String,
}

/// Online search state.
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchHit>,
    pub busy: bool,
    pub error: Option<String>,
}

/// SolRoute capture state.
#[derive(Clone, Debug, Default)]
pub struct RouteState {
    /// (id, name, is_default) of capturable outputs.
    pub sources: Vec<(String, String, bool)>,
    pub selected: Option<String>,
    /// Name of the device being captured, when running.
    pub running: Option<String>,
    pub seconds: f64,
    pub readings: LiveReadings,
    pub error: Option<String>,
}

/// Which view is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    #[default]
    Overview,
    Diff,
    Parts,
    Library,
    Search,
    Route,
}

/// A short message for the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub error: bool,
    pub text: String,
}

/// Everything the UI reads.
#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub library: Library,
    pub library_path: PathBuf,
    /// Session-only references (not in the library).
    pub session: Vec<RefEntry>,
    /// References compared together (DIFF / PARTS group), by id.
    pub group: Vec<String>,
    /// The reference that plays on B.
    pub active: Option<String>,
    /// Whether the active reference's audio is loaded and playable.
    pub active_ready: bool,
    pub active_seconds: f64,
    pub level_match: bool,
    pub ref_gain_db: f64,
    pub view: View,
    pub mix: Option<Profile>,
    pub mix_seconds: f64,
    pub readings: LiveReadings,
    pub group_stats: Option<GroupStats>,
    pub diff: Option<Diff>,
    pub search: SearchState,
    pub route: RouteState,
    pub notices: Vec<Notice>,
    pub busy: Vec<String>,
    /// True when running inside a host that never activated audio (shows a hint).
    pub audio_running: bool,
}

impl AppState {
    /// Every reference the UI can pick: library first, then this session's.
    #[must_use]
    pub fn all_refs(&self) -> Vec<RefEntry> {
        let mut v: Vec<RefEntry> = self
            .library
            .references
            .iter()
            .map(|r| RefEntry {
                id: r.id.clone(),
                title: r.title.clone(),
                artist: r.artist.clone(),
                origin: r.origin.clone(),
                profile: r.profile.clone(),
                in_library: true,
            })
            .collect();
        v.extend(
            self.session
                .iter()
                .filter(|s| !v.iter().any(|r| r.id == s.id))
                .cloned()
                .collect::<Vec<_>>(),
        );
        v
    }

    /// Finds a reference by id.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<RefEntry> {
        self.all_refs().into_iter().find(|r| r.id == id)
    }

    /// Adds a status message, keeping the last few.
    pub fn notify(&mut self, error: bool, text: impl Into<String>) {
        self.notices.push(Notice {
            error,
            text: text.into(),
        });
        if self.notices.len() > 6 {
            self.notices.remove(0);
        }
    }
}

/// What the plugin saves in the DAW project.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedState {
    pub version: u32,
    pub group: Vec<String>,
    pub active: Option<String>,
    pub listening_reference: bool,
    pub level_match: bool,
    pub view: View,
}

/// What the UI asks the worker to do.
#[derive(Clone, Debug)]
pub enum Command {
    ImportFiles(Vec<PathBuf>),
    Remove(String),
    Rename {
        id: String,
        title: String,
        artist: String,
    },
    ToggleInGroup(String),
    SetActive(String),
    SaveGroup(String),
    LoadGroup(String),
    DeleteGroup(String),
    ResetMix,
    SetLevelMatch(bool),
    SetView(View),
    SetOnlineSearch(bool),
    Search(String),
    Preview(SearchHit),
    SaveSession(String),
    RouteRefresh,
    RouteSelect(Option<String>),
    RouteStart,
    RouteStop,
    RouteUse {
        title: String,
    },
    ExportLibrary(PathBuf),
    ImportLibrary(PathBuf),
    SetTilt(f64),
    DismissNotices,
}
