//! BPM and key written into file names, as sample packs usually do:
//! "Loop_Dark_140bpm_Fm.wav", "128 BPM - Am - Chords.wav", "kick_C.wav".

use crate::keys;

/// Tokens of a file stem, split on separators.
fn tokens(stem: &str) -> Vec<String> {
    stem.split(['_', '-', ' ', '.', '(', ')', '[', ']', ',', '+'])
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn plausible_bpm(v: f64) -> Option<f64> {
    (40.0..=300.0).contains(&v).then_some(v)
}

/// BPM from a file stem: "140bpm", "140 BPM", "bpm140", "140_bpm".
#[must_use]
pub fn bpm(stem: &str) -> Option<f64> {
    let t = tokens(stem);
    for (i, tok) in t.iter().enumerate() {
        let lower = tok.to_ascii_lowercase();
        if let Some(n) = lower.strip_suffix("bpm").filter(|n| !n.is_empty())
            && let Some(v) = n.parse().ok().and_then(plausible_bpm)
        {
            return Some(v);
        }
        if let Some(n) = lower.strip_prefix("bpm").filter(|n| !n.is_empty())
            && let Some(v) = n.parse().ok().and_then(plausible_bpm)
        {
            return Some(v);
        }
        if lower == "bpm"
            && let Some(v) = i
                .checked_sub(1)
                .and_then(|j| t[j].parse().ok())
                .and_then(plausible_bpm)
                .or_else(|| {
                    t.get(i + 1)
                        .and_then(|n| n.parse().ok())
                        .and_then(plausible_bpm)
                })
        {
            return Some(v);
        }
    }
    None
}

/// Key from a file stem: a token that reads as a key ("Am", "F#min", "Cmaj", "8A").
/// A lone letter ("A", "C") counts only when it's the last token or followed by "maj"/"min",
/// since "A" is also a word.
#[must_use]
pub fn key(stem: &str) -> Option<String> {
    let t = tokens(stem);
    for (i, tok) in t.iter().enumerate() {
        let next = t.get(i + 1).map(|s| s.to_ascii_lowercase());
        if let Some(n) = &next
            && matches!(n.as_str(), "maj" | "major" | "min" | "minor")
            && tok.len() <= 2
            && let Some(k) = keys::normalize(&format!("{tok} {n}"))
        {
            return Some(k);
        }
        let is_last = i + 1 == t.len();
        let single = tok.len() == 1;
        // Camelot codes are too easy to confuse with take numbers ("1A"), so skip them here.
        let camelot_like = tok.chars().next().is_some_and(|c| c.is_ascii_digit());
        if camelot_like || (single && !is_last) {
            continue;
        }
        // Key tokens start with an upper-case note letter ("Am"), so words like "bass" don't match.
        if !tok.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            continue;
        }
        if let Some(k) = keys::normalize(tok) {
            return Some(k);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_common_names() {
        assert_eq!(bpm("Loop_Dark_140bpm_Fm"), Some(140.0));
        assert_eq!(key("Loop_Dark_140bpm_Fm").as_deref(), Some("F minor"));
        assert_eq!(bpm("128 BPM - Am - Chords"), Some(128.0));
        assert_eq!(key("128 BPM - Am - Chords").as_deref(), Some("A minor"));
        assert_eq!(key("Pad C maj 90").as_deref(), Some("C major"));
        assert_eq!(key("Bass_Eb").as_deref(), Some("Eb major"));
        assert_eq!(bpm("Snare 01"), None);
        assert_eq!(key("A big kick"), None);
        assert_eq!(key("bass dark"), None);
        assert_eq!(key("Kick 1A"), None);
        assert_eq!(bpm("BPM_95_Groove"), Some(95.0));
    }
}
