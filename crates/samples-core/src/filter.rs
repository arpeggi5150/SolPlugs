//! Search and filter: text, tags, genres, BPM range, key, rating, favorites, raw/edited, pack.

use crate::keys;
use crate::model::{Library, Sample};

/// Which versions to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Versions {
    #[default]
    All,
    Raws,
    Edited,
}

/// How to sort.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Newest,
    Name,
    Bpm,
    Key,
    Length,
    Rating,
}

/// A query. Empty fields don't filter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Words that must all appear in the name, path, tags, genres, key or comment.
    pub text: String,
    /// All of these tags.
    pub tags: Vec<String>,
    /// Any of these genres.
    pub genres: Vec<String>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    /// A key in any spelling ("Am", "A minor").
    pub key: Option<String>,
    pub min_rating: u8,
    pub favorites: bool,
    pub versions: Versions,
    /// Only members of this pack.
    pub pack: Option<String>,
    pub sort: Sort,
}

impl Query {
    /// True if `s` passes.
    #[must_use]
    pub fn matches(&self, lib: &Library, s: &Sample) -> bool {
        if self.favorites && !s.favorite {
            return false;
        }
        if s.rating < self.min_rating {
            return false;
        }
        match self.versions {
            Versions::Raws if s.version.is_edited() => return false,
            Versions::Edited if !s.version.is_edited() => return false,
            _ => {}
        }
        if let Some(p) = &self.pack
            && !lib.pack(p).is_some_and(|p| p.members.contains(&s.id))
        {
            return false;
        }
        if self.bpm_min.is_some() || self.bpm_max.is_some() {
            let Some(b) = s.bpm else { return false };
            if self.bpm_min.is_some_and(|m| b < m - 0.05)
                || self.bpm_max.is_some_and(|m| b > m + 0.05)
            {
                return false;
            }
        }
        if let Some(k) = self.key.as_deref().and_then(keys::parse)
            && s.key.as_deref().and_then(keys::parse) != Some(k)
        {
            return false;
        }
        if !self
            .tags
            .iter()
            .all(|t| s.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))
        {
            return false;
        }
        if !self.genres.is_empty()
            && !self
                .genres
                .iter()
                .any(|g| s.genres.iter().any(|x| x.eq_ignore_ascii_case(g)))
        {
            return false;
        }
        let text = self.text.trim();
        if !text.is_empty() {
            let hay = format!(
                "{} {} {} {} {} {} {}",
                s.name,
                s.path.to_string_lossy(),
                s.tags.join(" "),
                s.genres.join(" "),
                s.key.as_deref().unwrap_or(""),
                s.comment,
                s.file_type.label()
            )
            .to_lowercase();
            if !text
                .to_lowercase()
                .split_whitespace()
                .all(|w| hay.contains(w))
            {
                return false;
            }
        }
        true
    }

    /// Indices of matching samples, sorted.
    #[must_use]
    pub fn run(&self, lib: &Library) -> Vec<usize> {
        let mut out: Vec<usize> = lib
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| self.matches(lib, s))
            .map(|(i, _)| i)
            .collect();
        let pack_order = self.pack.as_deref().and_then(|p| lib.pack(p));
        let s = |i: &usize| &lib.samples[*i];
        match self.sort {
            Sort::Newest if pack_order.is_some() => {
                if let Some(p) = pack_order {
                    out.sort_by_key(|i| p.members.iter().position(|m| *m == lib.samples[*i].id));
                }
            }
            Sort::Newest => {
                out.sort_by(|a, b| s(b).added_unix.cmp(&s(a).added_unix).then(b.cmp(a)))
            }
            Sort::Name => out.sort_by_key(|i| s(i).name.to_lowercase()),
            Sort::Bpm => out.sort_by(|a, b| {
                s(a).bpm
                    .unwrap_or(f64::MAX)
                    .total_cmp(&s(b).bpm.unwrap_or(f64::MAX))
            }),
            Sort::Key => out.sort_by_key(|i| {
                s(i).key
                    .as_deref()
                    .and_then(keys::parse)
                    .map_or(99, |k| u16::from(k.minor) * 12 + u16::from(k.pitch))
            }),
            Sort::Length => out.sort_by(|a, b| s(a).duration_sec.total_cmp(&s(b).duration_sec)),
            Sort::Rating => out.sort_by_key(|i| std::cmp::Reverse(s(i).rating)),
        }
        out
    }
}
