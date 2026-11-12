//! The background worker: drains the mix, measures, loads references, searches, captures.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use solplugs_ref_core::diff::{DiffOptions, diff};
use solplugs_ref_core::group::group_stats;
use solplugs_ref_core::live::LiveAnalyzer;
use solplugs_ref_core::profile::{Profile, analyze};
use solplugs_ref_core::resample::resample;
use solplugs_ref_io::decode::{Audio, decode_file};
use solplugs_ref_io::library::{Library, Origin};

use crate::bridge::{Bridge, RefAudio};
use crate::state::{AppState, Command, RefEntry, SavedState, SearchHit};

/// Loudness the reference is matched to before the mix has been measured.
const FALLBACK_TARGET_LUFS: f64 = -14.0;
/// Longest SolRoute capture kept (seconds).
const MAX_CAPTURE_SECS: f64 = 180.0;
/// Decoded references kept in memory.
const CACHE: usize = 4;

pub(crate) enum Msg {
    Cmd(Command),
    Restore(SavedState),
    Quit,
}

enum Job {
    Imported {
        path: PathBuf,
        profile: Box<Profile>,
        audio: Audio,
    },
    Loaded {
        id: String,
        audio: Audio,
    },
    SearchDone(Result<Vec<SearchHit>, String>),
    Previewed {
        hit: SearchHit,
        profile: Box<Profile>,
        audio: Audio,
    },
    Captured {
        title: String,
        profile: Box<Profile>,
        audio: Audio,
    },
    Failed {
        what: String,
        error: String,
    },
}

struct Cached {
    id: String,
    source: Audio,
    /// Prepared at the host rate it was made for.
    prepared: Option<(f64, Arc<RefAudio>)>,
}

pub(crate) struct Worker {
    bridge: Arc<Bridge>,
    state: Arc<Mutex<AppState>>,
    rx: Receiver<Msg>,
    jobs_tx: Sender<Job>,
    jobs_rx: Receiver<Job>,
    mix: Option<LiveAnalyzer>,
    mix_rx: Option<rtrb::Consumer<f32>>,
    generation: u64,
    cache: Vec<Cached>,
    loading: Option<String>,
    route: Option<solplugs_route::Feed>,
    route_an: Option<LiveAnalyzer>,
    route_l: Vec<f32>,
    route_r: Vec<f32>,
    last_readings: Instant,
    last_profile: Instant,
    last_parts: Instant,
    last_lib_check: Instant,
    lib_mtime: Option<SystemTime>,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
}

fn file_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Reference".into())
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Worker {
    pub(crate) fn new(bridge: Arc<Bridge>, state: Arc<Mutex<AppState>>, rx: Receiver<Msg>) -> Self {
        let (jobs_tx, jobs_rx) = channel();
        let now = Instant::now();
        Self {
            bridge,
            state,
            rx,
            jobs_tx,
            jobs_rx,
            mix: None,
            mix_rx: None,
            generation: 0,
            cache: Vec::new(),
            loading: None,
            route: None,
            route_an: None,
            route_l: Vec::new(),
            route_r: Vec::new(),
            last_readings: now,
            last_profile: now,
            last_parts: now,
            last_lib_check: now,
            lib_mtime: None,
            scratch_l: Vec::new(),
            scratch_r: Vec::new(),
        }
    }

    fn st<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
        self.state.lock().ok().map(|mut s| f(&mut s))
    }

    fn notify(&self, error: bool, text: impl Into<String>) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        }
        self.st(|s| s.notify(error, text));
    }

    pub(crate) fn run(mut self) {
        self.load_library();
        self.refresh_routes();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(30)) {
                Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Msg::Cmd(c)) => self.command(c),
                Ok(Msg::Restore(saved)) => self.restore(saved),
                Err(RecvTimeoutError::Timeout) => {}
            }
            while let Ok(job) = self.jobs_rx.try_recv() {
                self.job_done(job);
            }
            self.tick();
        }
    }

    fn load_library(&mut self) {
        let Some(path) = self.st(|s| s.library_path.clone()) else {
            return;
        };
        match Library::load(&path) {
            Ok(lib) => {
                self.lib_mtime = mtime(&path);
                self.st(|s| {
                    s.library = lib;
                    let ids: Vec<String> = s.all_refs().into_iter().map(|r| r.id).collect();
                    s.group.retain(|g| ids.contains(g));
                });
            }
            Err(e) => self.notify(true, format!("Couldn't open the reference library: {e}")),
        }
    }

    fn save_library(&mut self) {
        let Some((lib, path)) = self.st(|s| (s.library.clone(), s.library_path.clone())) else {
            return;
        };
        if let Err(e) = lib.save(&path) {
            self.notify(true, format!("Couldn't save the reference library: {e}"));
        }
        self.lib_mtime = mtime(&path);
    }

    fn restore(&mut self, saved: SavedState) {
        self.st(|s| {
            s.group = saved.group;
            s.level_match = saved.level_match;
            s.view = saved.view;
        });
        if let Some(id) = saved.active {
            self.set_active(&id);
        }
    }

    #[allow(clippy::cognitive_complexity)]
    fn command(&mut self, c: Command) {
        match c {
            Command::ImportFiles(paths) => self.import(paths),
            Command::Remove(id) => {
                self.st(|s| {
                    s.library.remove(&id);
                    s.session.retain(|r| r.id != id);
                    s.group.retain(|g| *g != id);
                    if s.active.as_deref() == Some(id.as_str()) {
                        s.active = None;
                        s.active_ready = false;
                    }
                });
                self.cache.retain(|c| c.id != id);
                if self.bridge.reference().as_ref().is_some_and(|r| r.id == id) {
                    self.bridge.set_reference(None);
                }
                self.save_library();
            }
            Command::Rename { id, title, artist } => {
                self.st(|s| {
                    s.library.rename(&id, &title, &artist);
                    if let Some(r) = s.session.iter_mut().find(|r| r.id == id) {
                        r.title = title.clone();
                        r.artist = artist.clone();
                    }
                });
                self.save_library();
            }
            Command::ToggleInGroup(id) => {
                self.st(|s| {
                    if let Some(i) = s.group.iter().position(|g| *g == id) {
                        s.group.remove(i);
                    } else {
                        s.group.push(id);
                    }
                });
            }
            Command::SetActive(id) => self.set_active(&id),
            Command::SaveGroup(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    self.st(|s| {
                        let members = s
                            .group
                            .iter()
                            .filter(|id| s.library.get(id).is_some())
                            .cloned()
                            .collect();
                        s.library.group_mut(&name).members = members;
                    });
                    self.save_library();
                    self.notify(false, format!("Saved group \"{name}\""));
                }
            }
            Command::LoadGroup(name) => {
                self.st(|s| {
                    if let Some(g) = s.library.groups.iter().find(|g| g.name == name) {
                        s.group = g.members.clone();
                    }
                });
            }
            Command::DeleteGroup(name) => {
                self.st(|s| s.library.groups.retain(|g| g.name != name));
                self.save_library();
            }
            Command::ResetMix => {
                if let Some(m) = &mut self.mix {
                    m.reset();
                }
                self.st(|s| {
                    s.mix = None;
                    s.diff = None;
                    s.mix_seconds = 0.0;
                });
            }
            Command::SetLevelMatch(on) => {
                self.st(|s| s.level_match = on);
            }
            Command::SetView(v) => {
                self.st(|s| s.view = v);
            }
            Command::SetOnlineSearch(on) => {
                self.st(|s| s.library.settings.online_search = on);
                self.save_library();
            }
            Command::SetTilt(t) => {
                self.st(|s| s.library.settings.display_tilt_db_per_oct = t.clamp(0.0, 6.0));
                self.save_library();
            }
            Command::Search(q) => self.search(q),
            Command::Preview(hit) => self.preview(hit),
            Command::SaveSession(id) => self.save_session(&id),
            Command::RouteRefresh => self.refresh_routes(),
            Command::RouteSelect(id) => {
                self.st(|s| s.route.selected = id);
            }
            Command::RouteStart => self.route_start(),
            Command::RouteStop => self.route_stop(),
            Command::RouteUse { title } => self.route_use(title),
            Command::ExportLibrary(path) => {
                let lib = self.st(|s| s.library.clone()).unwrap_or_default();
                match lib.save(&path) {
                    Ok(()) => {
                        self.notify(false, format!("Exported the library to {}", path.display()))
                    }
                    Err(e) => self.notify(true, format!("Export failed: {e}")),
                }
            }
            Command::ImportLibrary(path) => match Library::load(&path) {
                Ok(other) => {
                    let n = self.st(|s| s.library.merge(other)).unwrap_or(0);
                    self.save_library();
                    self.notify(false, format!("Imported {n} references"));
                }
                Err(e) => self.notify(true, format!("Import failed: {e}")),
            },
            Command::DismissNotices => {
                self.st(|s| s.notices.clear());
            }
        }
    }

    fn busy(&self, what: &str, on: bool) {
        self.st(|s| {
            if on {
                s.busy.push(what.to_string());
            } else if let Some(i) = s.busy.iter().position(|b| b == what) {
                s.busy.remove(i);
            }
        });
    }

    fn import(&mut self, paths: Vec<PathBuf>) {
        let room = self
            .st(|s| {
                solplugs_ref_io::library::MAX_REFERENCES.saturating_sub(s.library.references.len())
            })
            .unwrap_or(0);
        if room == 0 {
            self.notify(
                true,
                "The library is full (60 references). Remove one first.",
            );
            return;
        }
        for path in paths.into_iter().take(room) {
            let tx = self.jobs_tx.clone();
            self.busy(&format!("Analysing {}", file_title(&path)), true);
            std::thread::spawn(move || {
                let job = match decode_file(&path) {
                    Ok(audio) => {
                        let profile = analyze(&audio.left, &audio.right, audio.sample_rate);
                        Job::Imported {
                            path,
                            profile: Box::new(profile),
                            audio,
                        }
                    }
                    Err(e) => Job::Failed {
                        what: format!("Analysing {}", file_title(&path)),
                        error: e.to_string(),
                    },
                };
                let _ = tx.send(job);
            });
        }
    }

    fn set_active(&mut self, id: &str) {
        let Some(entry) = self.st(|s| s.find(id)).flatten() else {
            return;
        };
        self.st(|s| {
            s.active = Some(id.to_string());
            s.active_ready = false;
            s.active_seconds = entry.profile.duration_secs;
        });
        if self.cache.iter().any(|c| c.id == id) {
            self.publish_active();
            return;
        }
        if self.loading.as_deref() == Some(id) {
            return;
        }
        let tx = self.jobs_tx.clone();
        let id = id.to_string();
        match entry.origin {
            Origin::File { path } => {
                self.loading = Some(id.clone());
                self.busy("Loading reference", true);
                std::thread::spawn(move || {
                    let job = match decode_file(&path) {
                        Ok(audio) => Job::Loaded { id, audio },
                        Err(e) => Job::Failed {
                            what: "Loading reference".into(),
                            error: format!("{}: {e}", path.display()),
                        },
                    };
                    let _ = tx.send(job);
                });
            }
            Origin::Deezer { track_id, .. } => self.stream_preview(id, track_id),
            Origin::Capture => {
                self.notify(true, "This capture's audio is no longer in memory; its analysis is still used in DIFF and PARTS.");
            }
        }
    }

    #[cfg(feature = "deezer")]
    fn stream_preview(&mut self, id: String, track_id: u64) {
        if !self
            .st(|s| s.library.settings.online_search)
            .unwrap_or(false)
        {
            self.notify(true, "Turn on online search to play Deezer previews.");
            return;
        }
        let tx = self.jobs_tx.clone();
        self.loading = Some(id.clone());
        self.busy("Loading reference", true);
        std::thread::spawn(move || {
            let job = solplugs_ref_io::deezer::track(track_id)
                .and_then(|t| solplugs_ref_io::deezer::fetch_preview(&t))
                .map(|audio| Job::Loaded { id, audio })
                .unwrap_or_else(|e| Job::Failed {
                    what: "Loading reference".into(),
                    error: e.to_string(),
                });
            let _ = tx.send(job);
        });
    }

    #[cfg(not(feature = "deezer"))]
    fn stream_preview(&mut self, _id: String, _track_id: u64) {
        self.notify(true, "This build has no online search.");
    }

    fn cache_audio(&mut self, id: &str, audio: Audio) {
        self.cache.retain(|c| c.id != id);
        self.cache.insert(
            0,
            Cached {
                id: id.to_string(),
                source: audio,
                prepared: None,
            },
        );
        // Never evict the reference that is playing.
        let playing = self.bridge.reference().as_ref().map(|r| r.id.clone());
        while self.cache.len() > CACHE {
            match self
                .cache
                .iter()
                .rposition(|c| Some(&c.id) != playing.as_ref())
            {
                Some(i) => {
                    self.cache.remove(i);
                }
                None => break,
            }
        }
    }

    /// Makes the active reference playable at the host rate and hands it to the audio thread.
    fn publish_active(&mut self) {
        let Some(active) = self.st(|s| s.active.clone()).flatten() else {
            return;
        };
        let rate = self.bridge.host_rate().unwrap_or(48_000.0);
        let Some(c) = self.cache.iter_mut().find(|c| c.id == active) else {
            return;
        };
        let ready = match &c.prepared {
            Some((r, a)) if (*r - rate).abs() < 0.5 => a.clone(),
            _ => {
                let a = Arc::new(RefAudio {
                    id: active.clone(),
                    left: resample(&c.source.left, c.source.sample_rate, rate),
                    right: resample(&c.source.right, c.source.sample_rate, rate),
                });
                c.prepared = Some((rate, a.clone()));
                a
            }
        };
        let secs = ready.left.len() as f64 / rate;
        self.bridge.set_reference(Some(ready));
        self.st(|s| {
            s.active_ready = true;
            s.active_seconds = secs;
        });
    }

    fn add_to_group_and_activate(&mut self, id: &str) {
        let first = self.st(|s| {
            if !s.group.iter().any(|g| g == id) {
                s.group.push(id.to_string());
            }
            s.active.is_none()
        });
        if first.unwrap_or(false) {
            self.set_active(id);
        }
    }

    fn job_done(&mut self, job: Job) {
        match job {
            Job::Imported {
                path,
                profile,
                audio,
            } => {
                let title = file_title(&path);
                self.busy(&format!("Analysing {title}"), false);
                let res = self.st(|s| {
                    s.library.add(
                        &title,
                        "",
                        Origin::File { path: path.clone() },
                        *profile.clone(),
                    )
                });
                match res {
                    Some(Ok(id)) => {
                        self.cache_audio(&id, audio);
                        self.save_library();
                        self.add_to_group_and_activate(&id);
                        self.notify(false, format!("Added \"{title}\""));
                    }
                    Some(Err(e)) => self.notify(true, e.to_string()),
                    None => {}
                }
            }
            Job::Loaded { id, audio } => {
                self.busy("Loading reference", false);
                if self.loading.as_deref() == Some(id.as_str()) {
                    self.loading = None;
                }
                self.cache_audio(&id, audio);
                if self
                    .st(|s| s.active.as_deref() == Some(id.as_str()))
                    .unwrap_or(false)
                {
                    self.publish_active();
                }
            }
            Job::SearchDone(res) => {
                self.st(|s| {
                    s.search.busy = false;
                    match res {
                        Ok(hits) => {
                            s.search.error = if hits.is_empty() {
                                Some("No previews found.".into())
                            } else {
                                None
                            };
                            s.search.results = hits;
                        }
                        Err(e) => s.search.error = Some(e),
                    }
                });
            }
            Job::Previewed {
                hit,
                profile,
                audio,
            } => {
                self.busy("Loading preview", false);
                let origin = Origin::Deezer {
                    track_id: hit.id,
                    link: hit.link.clone(),
                };
                let id = solplugs_ref_io::library::id_for(&origin, &hit.title);
                self.st(|s| {
                    s.session.retain(|r| r.id != id);
                    s.session.push(RefEntry {
                        id: id.clone(),
                        title: hit.title.clone(),
                        artist: hit.artist.clone(),
                        origin,
                        profile: *profile,
                        in_library: false,
                    });
                });
                self.cache_audio(&id, audio);
                self.add_to_group_and_activate(&id);
                self.set_active(&id);
                self.notify(
                    false,
                    format!("Loaded the preview of \"{}\" (not saved yet)", hit.title),
                );
            }
            Job::Captured {
                title,
                profile,
                audio,
            } => {
                self.busy("Analysing capture", false);
                let id = solplugs_ref_io::library::id_for(&Origin::Capture, &title);
                self.st(|s| {
                    s.session.push(RefEntry {
                        id: id.clone(),
                        title: title.clone(),
                        artist: "SolRoute".into(),
                        origin: Origin::Capture,
                        profile: *profile,
                        in_library: false,
                    });
                });
                self.cache_audio(&id, audio);
                self.add_to_group_and_activate(&id);
                self.set_active(&id);
                self.notify(false, format!("\"{title}\" is now a reference"));
            }
            Job::Failed { what, error } => {
                self.busy(&what, false);
                if what == "Loading reference" {
                    self.loading = None;
                }
                if what == "Loading preview" {
                    self.st(|s| s.search.busy = false);
                }
                self.notify(true, format!("{what} failed: {error}"));
            }
        }
    }

    fn save_session(&mut self, id: &str) {
        let Some(entry) = self
            .st(|s| s.session.iter().find(|r| r.id == id).cloned())
            .flatten()
        else {
            return;
        };
        let res = self.st(|s| {
            s.library.add(
                &entry.title,
                &entry.artist,
                entry.origin.clone(),
                entry.profile.clone(),
            )
        });
        match res {
            Some(Ok(new_id)) => {
                self.st(|s| {
                    s.session.retain(|r| r.id != id);
                    for g in &mut s.group {
                        if g == id {
                            *g = new_id.clone();
                        }
                    }
                    if s.active.as_deref() == Some(id) {
                        s.active = Some(new_id.clone());
                    }
                });
                if let Some(c) = self.cache.iter_mut().find(|c| c.id == id) {
                    c.id = new_id;
                }
                self.save_library();
                self.notify(
                    false,
                    format!("Saved \"{}\" to the library (analysis only)", entry.title),
                );
            }
            Some(Err(e)) => self.notify(true, e.to_string()),
            None => {}
        }
    }

    #[cfg(feature = "deezer")]
    fn search(&mut self, q: String) {
        if !self
            .st(|s| s.library.settings.online_search)
            .unwrap_or(false)
            || q.trim().is_empty()
        {
            return;
        }
        self.st(|s| {
            s.search.busy = true;
            s.search.error = None;
            s.search.query = q.clone();
        });
        let tx = self.jobs_tx.clone();
        std::thread::spawn(move || {
            let res = solplugs_ref_io::deezer::search(&q, 25)
                .map(|v| {
                    v.into_iter()
                        .map(|t| SearchHit {
                            id: t.id,
                            title: t.title,
                            artist: t.artist,
                            album: t.album,
                            duration_secs: t.duration_secs,
                            preview_url: t.preview_url,
                            link: t.link,
                        })
                        .collect()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Job::SearchDone(res));
        });
    }

    #[cfg(feature = "deezer")]
    fn preview(&mut self, hit: SearchHit) {
        if !self
            .st(|s| s.library.settings.online_search)
            .unwrap_or(false)
        {
            return;
        }
        let tx = self.jobs_tx.clone();
        self.busy("Loading preview", true);
        std::thread::spawn(move || {
            let track = solplugs_ref_io::deezer::Track {
                id: hit.id,
                title: hit.title.clone(),
                artist: hit.artist.clone(),
                album: hit.album.clone(),
                duration_secs: hit.duration_secs,
                preview_url: hit.preview_url.clone(),
                link: hit.link.clone(),
            };
            let job = match solplugs_ref_io::deezer::fetch_preview(&track) {
                Ok(audio) => {
                    let profile = analyze(&audio.left, &audio.right, audio.sample_rate);
                    Job::Previewed {
                        hit,
                        profile: Box::new(profile),
                        audio,
                    }
                }
                Err(e) => Job::Failed {
                    what: "Loading preview".into(),
                    error: e.to_string(),
                },
            };
            let _ = tx.send(job);
        });
    }

    #[cfg(not(feature = "deezer"))]
    fn search(&mut self, _q: String) {}

    #[cfg(not(feature = "deezer"))]
    fn preview(&mut self, _hit: SearchHit) {}

    fn refresh_routes(&mut self) {
        match solplugs_route::sources() {
            Ok(list) => {
                self.st(|s| {
                    s.route.sources = list
                        .into_iter()
                        .map(|d| (d.id, d.name, d.is_default))
                        .collect();
                    s.route.error = None;
                });
            }
            Err(e) => {
                self.st(|s| s.route.error = Some(e.to_string()));
            }
        }
    }

    fn route_start(&mut self) {
        self.route_stop();
        let sel = self.st(|s| s.route.selected.clone()).flatten();
        match solplugs_route::Feed::start(sel.as_deref()) {
            Ok(feed) => {
                self.route_an = Some(LiveAnalyzer::new(feed.sample_rate()));
                self.route_l.clear();
                self.route_r.clear();
                let name = feed.name().to_string();
                self.route = Some(feed);
                self.st(|s| {
                    s.route.running = Some(name);
                    s.route.error = None;
                    s.route.seconds = 0.0;
                });
            }
            Err(e) => {
                self.st(|s| s.route.error = Some(e.to_string()));
            }
        }
    }

    fn route_stop(&mut self) {
        self.route = None;
        self.st(|s| s.route.running = None);
    }

    fn route_use(&mut self, title: String) {
        let Some(feed) = &self.route else {
            self.notify(true, "Start a SolRoute capture first.");
            return;
        };
        if self.route_l.len() < (feed.sample_rate() * 3.0) as usize {
            self.notify(true, "Capture at least 3 seconds first.");
            return;
        }
        let audio = Audio {
            sample_rate: feed.sample_rate(),
            left: self.route_l.clone(),
            right: self.route_r.clone(),
        };
        let title = if title.trim().is_empty() {
            "SolRoute capture".to_string()
        } else {
            title.trim().to_string()
        };
        let tx = self.jobs_tx.clone();
        self.busy("Analysing capture", true);
        std::thread::spawn(move || {
            let profile = analyze(&audio.left, &audio.right, audio.sample_rate);
            let _ = tx.send(Job::Captured {
                title,
                profile: Box::new(profile),
                audio,
            });
        });
    }

    fn tick(&mut self) {
        let now = Instant::now();
        // Pick up a new mix ring after (re)activation.
        let g = self.bridge.generation();
        if g != self.generation {
            self.generation = g;
            self.mix_rx = self.bridge.mix_rx.lock().ok().and_then(|mut s| s.take());
            let rate = self.bridge.host_rate().unwrap_or(48_000.0);
            if self
                .mix
                .as_ref()
                .is_none_or(|m| (m.sample_rate() - rate).abs() > 0.5)
            {
                self.mix = Some(LiveAnalyzer::new(rate));
            }
            self.st(|s| s.audio_running = true);
            self.publish_active();
        }
        if let (Some(rx), Some(mix)) = (&mut self.mix_rx, &mut self.mix) {
            let n = rx.slots() / 2 * 2;
            if n > 0
                && let Ok(chunk) = rx.read_chunk(n)
            {
                self.scratch_l.clear();
                self.scratch_r.clear();
                let (a, b) = chunk.as_slices();
                let mut it = a.iter().chain(b.iter());
                while let (Some(&l), Some(&r)) = (it.next(), it.next()) {
                    self.scratch_l.push(l);
                    self.scratch_r.push(r);
                }
                chunk.commit_all();
                mix.process(&self.scratch_l, &self.scratch_r);
            }
        }
        if let (Some(feed), Some(an)) = (&mut self.route, &mut self.route_an) {
            let (rl, rr) = (&mut self.route_l, &mut self.route_r);
            let cap = (feed.sample_rate() * MAX_CAPTURE_SECS) as usize;
            feed.drain(|l, r| {
                an.process(l, r);
                rl.extend_from_slice(l);
                rr.extend_from_slice(r);
                if rl.len() > cap {
                    let excess = rl.len() - cap;
                    rl.drain(..excess);
                    rr.drain(..excess);
                }
            });
        }
        if now.duration_since(self.last_readings) >= Duration::from_millis(50) {
            self.last_readings = now;
            let readings = self.mix.as_ref().map(LiveAnalyzer::readings);
            let mix_secs = self.mix.as_ref().map_or(0.0, LiveAnalyzer::seconds);
            let route = self.route_an.as_ref().map(LiveAnalyzer::readings);
            let route_secs = self
                .route
                .as_ref()
                .map_or(0.0, |f| self.route_l.len() as f64 / f.sample_rate());
            self.st(|s| {
                if let Some(r) = readings {
                    s.readings = r;
                }
                s.mix_seconds = mix_secs;
                if let Some(r) = route {
                    s.route.readings = r;
                }
                s.route.seconds = route_secs;
            });
            self.update_match();
        }
        if now.duration_since(self.last_profile) >= Duration::from_millis(700) {
            self.last_profile = now;
            let with_parts = now.duration_since(self.last_parts) >= Duration::from_secs(4);
            if with_parts {
                self.last_parts = now;
            }
            let mix = self
                .mix
                .as_ref()
                .filter(|m| m.seconds() >= 1.0)
                .map(|m| m.profile(with_parts));
            self.st(|s| {
                if let Some(mut p) = mix {
                    if !with_parts && let Some(old) = &s.mix {
                        p.parts = old.parts.clone();
                    }
                    s.mix = Some(p);
                }
                let all = s.all_refs();
                let members: Vec<&Profile> = s
                    .group
                    .iter()
                    .filter_map(|id| all.iter().find(|r| &r.id == id))
                    .map(|r| &r.profile)
                    .collect();
                s.group_stats = group_stats(&members);
                s.diff = match (&s.mix, &s.group_stats) {
                    (Some(m), Some(g)) => Some(diff(m, g, DiffOptions::default())),
                    _ => None,
                };
            });
        }
        if now.duration_since(self.last_lib_check) >= Duration::from_secs(2) {
            self.last_lib_check = now;
            let path = self.st(|s| s.library_path.clone()).unwrap_or_default();
            let m = mtime(&path);
            if m.is_some() && m != self.lib_mtime {
                // Another instance (or Solara) changed the library.
                self.load_library();
            }
        }
    }

    fn update_match(&self) {
        let Some((on, active, mix_i, short)) = self.st(|s| {
            let active = s
                .active
                .as_ref()
                .and_then(|id| s.find(id))
                .and_then(|r| r.profile.integrated_lufs);
            (
                s.level_match,
                active,
                s.mix.as_ref().and_then(|m| m.integrated_lufs),
                s.readings.short_term_lufs,
            )
        }) else {
            return;
        };
        let gain = match (on, active) {
            (true, Some(ref_lufs)) => {
                let target = mix_i.or(short).unwrap_or(FALLBACK_TARGET_LUFS);
                (target - ref_lufs).clamp(
                    solplugs_ref_core::abswitch::MAX_MATCH_CUT_DB,
                    solplugs_ref_core::abswitch::MAX_MATCH_BOOST_DB,
                )
            }
            _ => 0.0,
        };
        self.bridge.set_reference_gain_db(gain as f32);
        self.st(|s| s.ref_gain_db = gain);
    }
}
