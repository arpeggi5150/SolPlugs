//! Compiles Song Sense's portable C++ analysis core (no JUCE) from a private checkout of
//! arpeggi5150/AI-Vault and links it. The source is never copied into SolPlugs.
//!
//! Where the core is looked for, first match wins:
//! 1. `SONGSENSE_CORE_DIR`: the `vst/core` folder itself.
//! 2. `AI-Vault/programs/VSTs/song-sense/vst/core` next to the SolPlugs checkout
//!    (for example `C:\dev\AI-Vault` beside `C:\dev\SolPlugs`).
//!
//! Without it the crate still builds; [`solplugs_songsense::available`] is then false and
//! BPM/key detection is skipped (no second detector is substituted).

use std::path::{Path, PathBuf};

/// The files of the portable build, as Song Sense's WebAssembly build compiles them.
const SOURCES: &[&str] = &[
    "Analysis.cpp",
    "Beats.cpp",
    "DspPortable.cpp",
    "Explain.cpp",
    "Exports.cpp",
    "Keys.cpp",
    "Meter.cpp",
    "Segments.cpp",
];

fn find_core() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SONGSENSE_CORE_DIR") {
        return Some(PathBuf::from(dir));
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    // crates/songsense -> workspace root -> its parent folder.
    let beside = manifest.parent()?.parent()?.parent()?;
    for name in ["AI-Vault", "ai-vault"] {
        let dir = beside.join(name).join("programs/VSTs/song-sense/vst/core");
        if dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

fn complete(dir: &Path) -> bool {
    SOURCES.iter().all(|f| dir.join(f).is_file()) && dir.join("Types.h").is_file()
}

fn main() {
    println!("cargo:rerun-if-env-changed=SONGSENSE_CORE_DIR");
    println!("cargo:rerun-if-changed=shim/shim.cpp");
    println!("cargo:rustc-check-cfg=cfg(songsense_linked)");
    let Some(dir) = find_core().filter(|d| complete(d)) else {
        println!(
            "cargo:warning=Song Sense core not found: BPM/key detection is off. Set SONGSENSE_CORE_DIR to AI-Vault/programs/VSTs/song-sense/vst/core."
        );
        return;
    };
    for f in SOURCES
        .iter()
        .chain(["Types.h", "Internal.h", "Dsp.h", "Dsp.cpp", "Stats.h"].iter())
    {
        println!("cargo:rerun-if-changed={}", dir.join(f).display());
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .include(&dir)
        .define("SS_NO_JUCE", None)
        .warnings(false)
        .file("shim/shim.cpp");
    if build.get_compiler().is_like_msvc() {
        build.flag("/EHsc").flag("/utf-8").flag("/bigobj");
    }
    for f in SOURCES {
        build.file(dir.join(f));
    }
    // Always optimise: the detector is far too slow unoptimised.
    build.opt_level(2);
    build.compile("songsense");
    println!("cargo:rustc-cfg=songsense_linked");
}
