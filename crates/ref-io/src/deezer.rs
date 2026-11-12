//! Online search through the Deezer public API (search and 30-second previews).
//!
//! Terms we follow (developers.deezer.com/guidelines and /termsofuse, checked 2026-11-12):
//! - Only the 30-second preview MP3s are used, never full tracks.
//! - "Local Storage/Offline Storage of audio data is strictly forbidden": previews are fetched
//!   into memory, decoded, analysed and dropped with the session. Nothing here writes audio
//!   to disk. The library stores the analysis and the track id only, and a preview is streamed
//!   again when the user wants to hear it.
//! - The API is for non-commercial use, so online search is off by default
//!   ([`crate::library::Settings::online_search`]) until Deezer approves our use.
//! - Results show "Preview from Deezer" and link back to the track.

use std::time::Duration;

use serde::Deserialize;

use crate::decode::{Audio, DecodeError, decode_bytes};

const API: &str = "https://api.deezer.com";
/// Previews are ~0.5 MB; anything far larger is not a preview.
const MAX_PREVIEW_BYTES: u64 = 4 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;
const MAX_QUERY_CHARS: usize = 200;

/// One search hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u32,
    /// Signed preview URL (expires; fetch [`track`] again for a fresh one).
    pub preview_url: String,
    /// The track's page on Deezer, for attribution.
    pub link: String,
}

/// Why a request failed.
#[derive(Debug, thiserror::Error)]
pub enum DeezerError {
    #[error("online search is turned off")]
    Disabled,
    #[error("couldn't reach Deezer: {0}")]
    Network(String),
    #[error("Deezer answered with something unexpected: {0}")]
    Response(String),
    #[error("this track has no preview")]
    NoPreview,
    #[error("the preview couldn't be decoded: {0}")]
    Decode(#[from] DecodeError),
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<RawTrack>,
    error: Option<RawError>,
}

#[derive(Deserialize)]
struct RawError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct RawTrack {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    duration: u32,
    #[serde(default)]
    preview: String,
    #[serde(default)]
    link: String,
    artist: Option<RawName>,
    album: Option<RawTitle>,
    error: Option<RawError>,
}

#[derive(Deserialize)]
struct RawName {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawTitle {
    #[serde(default)]
    title: String,
}

impl From<RawTrack> for Track {
    fn from(t: RawTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist.map(|a| a.name).unwrap_or_default(),
            album: t.album.map(|a| a.title).unwrap_or_default(),
            duration_secs: t.duration,
            preview_url: t.preview,
            link: t.link,
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent("SolPlugs-Reference/0.1")
        .build()
        .into()
}

fn net(e: impl std::fmt::Display) -> DeezerError {
    DeezerError::Network(e.to_string())
}

/// Percent-encodes a query string value.
#[must_use]
pub fn encode_query(q: &str) -> String {
    let mut out = String::new();
    for b in q.chars().take(MAX_QUERY_CHARS).collect::<String>().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, DeezerError> {
    let mut resp = agent().get(url).call().map_err(net)?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(net)?;
    serde_json::from_slice(&bytes).map_err(|e| DeezerError::Response(e.to_string()))
}

/// Parses a search response body (split out for tests).
pub fn parse_search(body: &[u8]) -> Result<Vec<Track>, DeezerError> {
    let r: SearchResponse =
        serde_json::from_slice(body).map_err(|e| DeezerError::Response(e.to_string()))?;
    if let Some(e) = r.error {
        return Err(DeezerError::Response(e.message));
    }
    Ok(r.data
        .into_iter()
        .map(Track::from)
        .filter(|t| !t.preview_url.is_empty())
        .collect())
}

/// Searches tracks. Only tracks with a preview are returned.
pub fn search(query: &str, limit: u32) -> Result<Vec<Track>, DeezerError> {
    let url = format!(
        "{API}/search/track?q={}&limit={}",
        encode_query(query.trim()),
        limit.clamp(1, 50)
    );
    let mut resp = agent().get(&url).call().map_err(net)?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(net)?;
    parse_search(&bytes)
}

/// Looks a track up again (for a fresh preview URL).
pub fn track(id: u64) -> Result<Track, DeezerError> {
    let t: RawTrack = get_json(&format!("{API}/track/{id}"))?;
    if let Some(e) = t.error {
        return Err(DeezerError::Response(e.message));
    }
    Ok(t.into())
}

/// Streams a track's preview into memory and decodes it. The bytes are never written to disk.
pub fn fetch_preview(t: &Track) -> Result<Audio, DeezerError> {
    let url = if t.preview_url.is_empty() {
        track(t.id)?.preview_url
    } else {
        t.preview_url.clone()
    };
    if !url.starts_with("https://") {
        return Err(DeezerError::NoPreview);
    }
    let mut resp = agent().get(&url).call().map_err(net)?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_PREVIEW_BYTES)
        .read_to_vec()
        .map_err(net)?;
    Ok(decode_bytes(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results_and_skips_tracks_without_previews() {
        let body = br#"{"data":[
            {"id":3135556,"title":"Harder, Better, Faster, Stronger","duration":224,
             "preview":"https://cdnt-preview.dzcdn.net/api/1/1/a/b/c/0/x.mp3?hdnea=exp=1",
             "link":"https://www.deezer.com/track/3135556",
             "artist":{"name":"Daft Punk"},"album":{"title":"Discovery"}},
            {"id":2,"title":"No preview","preview":"","artist":{"name":"X"}}],"total":2}"#;
        let hits = parse_search(body).unwrap_or_default();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].artist, "Daft Punk");
        assert_eq!(hits[0].album, "Discovery");
        assert_eq!(hits[0].duration_secs, 224);
    }

    #[test]
    fn api_errors_surface() {
        let body = br#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#;
        assert!(matches!(parse_search(body), Err(DeezerError::Response(m)) if m.contains("Quota")));
    }

    #[test]
    fn queries_are_encoded() {
        assert_eq!(encode_query("AC/DC & co"), "AC%2FDC%20%26%20co");
        assert_eq!(encode_query("é"), "%C3%A9");
    }

    /// Live check against the real API: `cargo test -p solplugs-ref-io -- --ignored deezer_live`.
    #[test]
    #[ignore = "needs network access to api.deezer.com"]
    #[allow(clippy::expect_used)]
    fn deezer_live_search_returns_a_playable_preview() {
        let hits = search("daft punk around the world", 5).expect("search");
        assert!(!hits.is_empty(), "no results");
        let t = &hits[0];
        println!("first hit: {} - {} (id {})", t.artist, t.title, t.id);
        let audio = fetch_preview(t).expect("preview");
        println!(
            "preview: {:.1} s at {} Hz",
            audio.seconds(),
            audio.sample_rate
        );
        assert!(audio.seconds() > 20.0 && audio.seconds() < 40.0);
        let peak = audio.left.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.01, "silent preview");
    }
}
