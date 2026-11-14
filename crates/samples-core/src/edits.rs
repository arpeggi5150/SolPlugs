//! Edited versions: every tool result and every saved selection becomes a new sample in the
//! library's edits folder, linked to the sample it came from and to the raw at the root.

use std::path::PathBuf;

use crate::audio::{self, Audio, WavDepth};
use crate::model::{FileType, Id, Library, Sample, Version, now_unix};
use crate::peaks::Peaks;
use crate::store::Store;
use crate::tools::{Range, Tool};

/// Which audio a saved selection holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Render {
    /// The dry audio of the range, exactly as in the file.
    PreFx,
    /// The range rendered through the sample's processing (EQ, gain, fades, normalize).
    PostFx,
    /// Both files at once, both linked to the source.
    Both,
}

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    /// Apply a tool to the whole sample's audio.
    Tool(Tool),
    /// Save a time range as a new sample.
    Selection { range: Range, render: Render },
}

/// Why an edit couldn't be saved.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("that sample isn't in the library")]
    NoSample,
    #[error("the result would be empty (select a longer range)")]
    Empty,
    #[error("can't write the edit: {0}")]
    Io(#[from] std::io::Error),
}

fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = cleaned.trim().trim_matches('.');
    let t: String = t.chars().take(60).collect();
    if t.is_empty() { "sample".into() } else { t }
}

/// Short label used in file and display names.
fn label(edit: &Edit) -> &'static str {
    match edit {
        Edit::Tool(Tool::Trim(_)) => "trim",
        Edit::Tool(Tool::TrimSilence) => "trimmed",
        Edit::Tool(Tool::FadeIn { .. }) => "fade in",
        Edit::Tool(Tool::FadeOut { .. }) => "fade out",
        Edit::Tool(Tool::Reverse(_)) => "reversed",
        Edit::Tool(Tool::Normalize(_)) => "normalized",
        Edit::Selection { .. } => "cut",
    }
}

/// Writes `audio` as a new edited version of `source` and adds it to the library.
fn add_version(
    lib: &mut Library,
    store: &Store,
    source: &Sample,
    audio: &Audio,
    tag: &str,
    edit: String,
    post_fx: bool,
) -> Result<Id, EditError> {
    if audio.frames() == 0 {
        return Err(EditError::Empty);
    }
    let id = lib.new_id();
    let root = match &source.version {
        Version::Edited { root, .. } => root.clone(),
        Version::Raw => source.id.clone(),
    };
    let dir = store.edits();
    std::fs::create_dir_all(&dir)?;
    let name = format!("{} ({tag})", source.name);
    let path: PathBuf = dir.join(format!("{} {id}.wav", safe_name(&name)));
    let float = source.bit_depth.is_none() && source.file_type != FileType::Mp3;
    let depth = WavDepth::like(source.bit_depth, float && source.bit_depth == Some(32));
    audio::write_wav(&path, audio, depth)?;
    let mut s = source.clone();
    s.id = id.clone();
    s.path = path;
    s.name = name;
    s.version = Version::Edited {
        parent: source.id.clone(),
        root,
        edit,
    };
    s.file_type = FileType::Wav;
    s.bit_depth = Some(depth.bits());
    s.channels = audio.channels.len() as u8;
    s.sample_rate = audio.sample_rate.round() as u32;
    s.duration_sec = audio.seconds();
    s.peaks = Peaks::of(audio);
    s.added_unix = now_unix();
    s.favorite = false;
    s.missing = false;
    if post_fx {
        // The processing is baked in now; leave the new file's own chain neutral.
        s.fx = Default::default();
    }
    lib.samples.push(s);
    Ok(id)
}

/// Saves an edit of sample `source_id`, whose decoded audio is `audio`. Returns the new
/// sample ids (two for [`Render::Both`]).
pub fn save(
    lib: &mut Library,
    store: &Store,
    source_id: &str,
    audio: &Audio,
    edit: Edit,
) -> Result<Vec<Id>, EditError> {
    let source = lib.sample(source_id).cloned().ok_or(EditError::NoSample)?;
    let tag = label(&edit);
    match edit {
        Edit::Tool(tool) => {
            let out = tool.apply(audio).ok_or(EditError::Empty)?;
            let what = tool.describe(audio.sample_rate);
            Ok(vec![add_version(
                lib, store, &source, &out, tag, what, false,
            )?])
        }
        Edit::Selection { range, render } => {
            let r = range.clamp(audio);
            if r.is_empty() {
                return Err(EditError::Empty);
            }
            let dry = audio.slice(r.start, r.end);
            let sr = audio.sample_rate.max(1.0);
            let span = format!("{:.2}-{:.2} s", r.start as f64 / sr, r.end as f64 / sr);
            let mut ids = Vec::new();
            if matches!(render, Render::PreFx | Render::Both) {
                ids.push(add_version(
                    lib,
                    store,
                    &source,
                    &dry,
                    "cut, pre FX",
                    format!("Selection {span}, Pre FX (dry)"),
                    false,
                )?);
            }
            if matches!(render, Render::PostFx | Render::Both) {
                let wet = source.fx.render(&dry);
                ids.push(add_version(
                    lib,
                    store,
                    &source,
                    &wet,
                    "cut, post FX",
                    format!("Selection {span}, Post FX ({})", source.fx.describe()),
                    true,
                )?);
            }
            Ok(ids)
        }
    }
}
