//! Import: find audio files in a folder and read what's known about each one.
//!
//! Files are never moved or copied. Each file gives a [`Sample`] with its tags, stream
//! properties, waveform overview, artwork, and BPM/key from (in order) its tags, its name,
//! or Song Sense's detector.

use std::path::{Path, PathBuf};

use crate::audio::{self, Audio};
use crate::model::{FileType, Sample, Source, Version, now_unix};
use crate::peaks::Peaks;
use crate::{art, names, tags};

/// Deepest folder nesting followed on import.
pub const MAX_DEPTH: usize = 16;

/// Every supported audio file under `folder` (or `folder` itself if it's a file), sorted.
#[must_use]
pub fn collect(folder: &Path) -> Vec<PathBuf> {
    if folder.is_file() {
        return supported(folder)
            .then(|| folder.to_path_buf())
            .into_iter()
            .collect();
    }
    let mut out: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && supported(e.path()))
        .map(walkdir::DirEntry::into_path)
        .collect();
    out.sort();
    out
}

/// True for a file extension we import.
#[must_use]
pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(FileType::from_ext)
        .is_some()
}

/// FNV-1a of a string, for stable file names.
#[must_use]
pub fn hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Genres named by a path's folders ("Samples/Trap/808s/x.wav" gives "Trap").
fn folder_genres(path: &Path, known: &[String]) -> Vec<String> {
    let norm = |s: &str| {
        let s: String = s
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '&' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        format!(" {} ", s.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let mut out = Vec::new();
    for comp in path.parent().into_iter().flat_map(Path::components) {
        let c = norm(&comp.as_os_str().to_string_lossy());
        for g in known {
            if c.contains(&norm(g)) && !out.contains(g) {
                out.push(g.clone());
            }
        }
    }
    out
}

/// Fills BPM and key from the file name, then Song Sense, where the tags left them empty.
pub fn fill_bpm_key(s: &mut Sample, audio: Option<&Audio>) {
    fill_from_name(s);
    if let Some(a) = audio {
        fill_detected(s, a);
    }
}

/// BPM and key from the file name, where empty.
pub fn fill_from_name(s: &mut Sample) {
    let stem = s
        .path
        .file_stem()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    if s.bpm.is_none()
        && let Some(b) = names::bpm(&stem)
    {
        s.bpm = Some(b);
        s.bpm_source = Source::Name;
    }
    if s.key.is_none()
        && let Some(k) = names::key(&stem)
    {
        s.key = Some(k);
        s.key_source = Source::Name;
    }
}

/// BPM and key from Song Sense, where still empty.
/// A tempo needs at least this many beats in the file before it is believable.
pub const MIN_BEATS: f64 = 4.0;

/// Whether a detected tempo is believable for a file this long. Short one-shots and noise
/// otherwise come back at odd tempos like 32 BPM.
pub fn tempo_plausible(bpm: f64, seconds: f64) -> bool {
    (40.0..=300.0).contains(&bpm) && seconds * bpm / 60.0 >= MIN_BEATS
}

pub fn fill_detected(s: &mut Sample, a: &Audio) {
    if (s.bpm.is_none() || s.key.is_none())
        && let Some(d) = solplugs_songsense::detect(&a.mono(), a.sample_rate)
    {
        // Song Sense calls a reading below 0.65 "low confidence"; leave those blank.
        if s.bpm.is_none()
            && let Some(b) = d.bpm.filter(|&b| {
                d.bpm_confidence >= solplugs_songsense::LOW_CONFIDENCE
                    && tempo_plausible(b, a.seconds())
            })
        {
            s.bpm = Some((b * 10.0).round() / 10.0);
            s.bpm_source = Source::Detected;
        }
        if s.key.is_none()
            && d.key_confidence >= solplugs_songsense::LOW_CONFIDENCE
            && let Some(k) = d.key.as_deref().and_then(crate::keys::normalize)
        {
            s.key = Some(k);
            s.key_source = Source::Detected;
        }
    }
}

/// Reads one file into a sample (id left empty for the library to assign).
///
/// `known_genres` lets folder names like "Trap" become genre tags; `art_dir` receives the
/// artwork thumbnail.
pub fn probe(path: &Path, known_genres: &[String], art_dir: &Path) -> Result<Sample, String> {
    let file_type = std::fs::File::open(path)
        .and_then(|mut f| {
            let mut head = [0u8; 12];
            let n = std::io::Read::read(&mut f, &mut head)?;
            Ok(audio::sniff(&head[..n]))
        })
        .map_err(|e| e.to_string())?
        .or_else(|| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(FileType::from_ext)
        })
        .ok_or("not an audio file")?;
    let info = tags::read(path);
    let decoded = audio::decode_file(path).map_err(|e| e.to_string())?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sample".into());
    let mut genres = info.genres.clone();
    for g in folder_genres(path, known_genres) {
        if !genres.iter().any(|x| x.eq_ignore_ascii_case(&g)) {
            genres.push(g);
        }
    }
    let key_tag = path.to_string_lossy();
    let art = info
        .picture
        .as_deref()
        .and_then(|bytes| art::save_thumb(bytes, art_dir, &hash(&key_tag)))
        .or_else(|| {
            let folder = path.parent()?;
            let img = art::folder_image(folder)?;
            let name = format!("dir-{}", hash(&img.to_string_lossy()));
            let cached = art_dir.join(format!("{name}.png"));
            if cached.is_file() {
                Some(cached)
            } else {
                art::thumb_from_file(&img, art_dir, &name)
            }
        });
    let mut s = Sample {
        id: String::new(),
        path: path.to_path_buf(),
        name,
        version: Version::Raw,
        file_type,
        sample_rate: info
            .sample_rate
            .unwrap_or(decoded.sample_rate.round() as u32),
        bit_depth: info.bit_depth,
        channels: info.channels.unwrap_or(decoded.channels.len() as u8),
        duration_sec: decoded.seconds(),
        bpm: info.bpm,
        bpm_source: if info.bpm.is_some() {
            Source::Tag
        } else {
            Source::None
        },
        key: info.key.clone(),
        key_source: if info.key.is_some() {
            Source::Tag
        } else {
            Source::None
        },
        tags: Vec::new(),
        genres,
        rating: 0,
        favorite: false,
        added_unix: now_unix(),
        artist: info.artist.unwrap_or_default(),
        comment: info.comment.unwrap_or_default(),
        art,
        peaks: Peaks::of(&decoded),
        fx: Default::default(),
        missing: false,
    };
    if let Some(t) = info.title
        && t.len() > 2
    {
        // Sample packs often leave a junk title; keep the file name unless the title is real.
        if !t.eq_ignore_ascii_case(&s.name) && !t.chars().all(|c| c.is_ascii_digit()) {
            s.comment = if s.comment.is_empty() {
                t
            } else {
                format!("{t} · {}", s.comment)
            };
        }
    }
    fill_bpm_key(&mut s, Some(&decoded));
    Ok(s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tempo_needs_enough_beats() {
        assert!(!super::tempo_plausible(32.0, 2.0));
        assert!(!super::tempo_plausible(140.0, 1.5));
        assert!(super::tempo_plausible(140.0, 2.0));
        assert!(super::tempo_plausible(85.0, 5.0));
        assert!(!super::tempo_plausible(400.0, 10.0));
    }

    use super::*;

    #[test]
    fn folder_names_give_genres() {
        let known = vec![
            "Trap".to_string(),
            "House".to_string(),
            "Hip Hop".to_string(),
        ];
        let g = folder_genres(Path::new("/s/My_Trap_Kit/808s/a.wav"), &known);
        assert_eq!(g, vec!["Trap".to_string()]);
        assert_eq!(
            folder_genres(Path::new("/s/Hip-Hop/a.wav"), &known),
            vec!["Hip Hop".to_string()]
        );
        assert!(folder_genres(Path::new("/s/Warehouse/a.wav"), &known).is_empty());
        let lofi = vec!["Lo-Fi".to_string()];
        assert_eq!(folder_genres(Path::new("/s/Lo-Fi Dust/a.wav"), &lofi), lofi);
        assert_eq!(
            folder_genres(Path::new("/s/lofi/a.wav"), &lofi),
            Vec::<String>::new()
        );
    }
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub added: Vec<String>,
    /// Files already in the library (left as they are).
    pub known: usize,
    /// Files that couldn't be read, with why.
    pub failed: Vec<(PathBuf, String)>,
}

/// Imports `paths` into `lib`. `progress(done, total, path)` is called before each file;
/// returning false stops the import.
pub fn import(
    lib: &mut crate::model::Library,
    store: &crate::store::Store,
    paths: &[PathBuf],
    mut progress: impl FnMut(usize, usize, &Path) -> bool,
) -> Report {
    let genres: Vec<String> = lib.genres.iter().map(|g| g.name.clone()).collect();
    let mut report = Report::default();
    for (i, p) in paths.iter().enumerate() {
        if !progress(i, paths.len(), p) {
            break;
        }
        if lib.by_path(p).is_some() {
            report.known += 1;
            continue;
        }
        // A panic anywhere in one file's import (decoders, tags, images) skips that file only.
        let probed = std::panic::catch_unwind(|| probe(p, &genres, &store.art()))
            .unwrap_or_else(|_| Err("this file crashed the reader".into()));
        match probed {
            Ok(mut s) => {
                s.id = lib.new_id();
                report.added.push(s.id.clone());
                lib.samples.push(s);
            }
            Err(e) => report.failed.push((p.clone(), e)),
        }
    }
    report
}
