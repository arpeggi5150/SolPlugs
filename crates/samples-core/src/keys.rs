//! Musical keys in one spelling, the one Song Sense uses ("C major", "F# minor"), so keys from
//! tags, file names, Song Sense and the user all compare equal.

const MAJOR: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const MINOR: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B",
];

/// A key: pitch class (0 = C) and mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub pitch: u8,
    pub minor: bool,
}

impl Key {
    /// Song Sense's spelling.
    #[must_use]
    pub fn name(self) -> String {
        let p = usize::from(self.pitch % 12);
        if self.minor {
            format!("{} minor", MINOR[p])
        } else {
            format!("{} major", MAJOR[p])
        }
    }

    /// Short form for cards ("C", "F#m").
    #[must_use]
    pub fn short(self) -> String {
        let p = usize::from(self.pitch % 12);
        if self.minor {
            format!("{}m", MINOR[p])
        } else {
            MAJOR[p].to_string()
        }
    }

    /// All 24 keys, majors then minors.
    #[must_use]
    pub fn all() -> Vec<Self> {
        (0..2)
            .flat_map(|m| {
                (0..12).map(move |p| Self {
                    pitch: p,
                    minor: m == 1,
                })
            })
            .collect()
    }
}

fn note(s: &str) -> Option<(u8, usize)> {
    let b = s.as_bytes();
    let base = match b.first()?.to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let mut used = 1;
    let mut p: i32 = base;
    if let Some(&c) = b.get(1) {
        if c == b'#' {
            p += 1;
            used = 2;
        } else if c == b'b' {
            p -= 1;
            used = 2;
        }
    }
    if s[1..].starts_with('♯') {
        p += 1;
        used = 1 + '♯'.len_utf8();
    } else if s[1..].starts_with('♭') {
        p -= 1;
        used = 1 + '♭'.len_utf8();
    }
    Some((p.rem_euclid(12) as u8, used))
}

/// Camelot wheel ("8A" = A minor, "8B" = C major).
fn camelot(s: &str) -> Option<Key> {
    let s = s.trim();
    let (num, letter) = s.split_at(s.len().checked_sub(1)?);
    let n: u8 = num.parse().ok()?;
    if !(1..=12).contains(&n) {
        return None;
    }
    // 8B = C major; each step up is a fifth (+7 semitones).
    let major_pitch = ((u32::from(n) + 12 - 8) * 7 % 12) as u8;
    match letter {
        "B" | "b" => Some(Key {
            pitch: major_pitch,
            minor: false,
        }),
        "A" | "a" => Some(Key {
            pitch: (major_pitch + 9) % 12,
            minor: true,
        }),
        _ => None,
    }
}

/// Parses a key the way tags and people write it: "Am", "A min", "A minor", "C", "Cmaj",
/// "F#m", "Bbmin", "8A" (Camelot). `None` for anything else.
#[must_use]
pub fn parse(s: &str) -> Option<Key> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(k) = camelot(s) {
        return Some(k);
    }
    let (pitch, used) = note(s)?;
    let rest = s[used..].trim().to_ascii_lowercase();
    let minor = match rest.as_str() {
        "" | "maj" | "major" | "ma" | "dur" => false,
        "m" | "min" | "minor" | "mi" | "moll" | "-" => true,
        _ => return None,
    };
    // A bare lower-case "m" after the note means minor; "M" alone would also be major.
    Some(Key { pitch, minor })
}

/// Normalises any key spelling to Song Sense's ("A minor"). Unknown text gives `None`.
#[must_use]
pub fn normalize(s: &str) -> Option<String> {
    parse(s).map(Key::name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_agree() {
        for s in ["Am", "A min", "A minor", "Amin", "8A", "a minor"] {
            assert_eq!(normalize(s).as_deref(), Some("A minor"), "{s}");
        }
        for s in ["C", "Cmaj", "C major", "8B"] {
            assert_eq!(normalize(s).as_deref(), Some("C major"), "{s}");
        }
        assert_eq!(normalize("F#m").as_deref(), Some("F# minor"));
        assert_eq!(normalize("Gbm").as_deref(), Some("F# minor"));
        assert_eq!(normalize("C#").as_deref(), Some("Db major"));
        assert_eq!(normalize("Bbmin").as_deref(), Some("Bb minor"));
        assert_eq!(normalize("5A").as_deref(), Some("C minor"));
        assert_eq!(normalize("1B").as_deref(), Some("B major"));
        assert_eq!(normalize("hello"), None);
        assert_eq!(normalize("Cowbell"), None);
    }

    #[test]
    fn every_key_round_trips() {
        for k in Key::all() {
            assert_eq!(parse(&k.name()), Some(k));
            assert_eq!(parse(&k.short()), Some(k));
        }
    }
}
