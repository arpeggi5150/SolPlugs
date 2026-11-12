//! Developer tasks.
//!
//! - `cargo xtask bundle [--debug]`: builds the plugin and lays out `dist/` with
//!   `SolPlugs Reference.vst3` (bundle folder) and `SolPlugs Reference.clap`.
//! - `cargo xtask install [--debug]`: bundles, then copies both into the per-user plugin
//!   folders (`%LOCALAPPDATA%\Programs\Common\VST3` and `...\CLAP` on Windows).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PACKAGE: &str = "solplugs-reference";
const LIB: &str = "solplugs_reference";
const NAME: &str = "SolPlugs Reference";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn dll_name() -> String {
    if cfg!(windows) {
        format!("{LIB}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{LIB}.dylib")
    } else {
        format!("lib{LIB}.so")
    }
}

fn vst3_arch_dir() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "x86_64-win"
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        "arm64-win"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else {
        "x86_64-linux"
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))
}

fn bundle(release: bool) -> Result<(PathBuf, PathBuf), String> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root()).args(["build", "-p", PACKAGE]);
    if release {
        cmd.arg("--release");
    }
    let status = cmd.status().map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let built = target
        .join(if release { "release" } else { "debug" })
        .join(dll_name());
    let dist = root().join("dist");
    let ext = if cfg!(windows) {
        "vst3"
    } else if cfg!(target_os = "macos") {
        ""
    } else {
        "so"
    };
    let vst3_bin = if cfg!(target_os = "macos") {
        NAME.to_string()
    } else {
        format!("{NAME}.{ext}")
    };
    let vst3 = dist.join(format!("{NAME}.vst3"));
    if vst3.exists() {
        std::fs::remove_dir_all(&vst3).map_err(|e| format!("clean {}: {e}", vst3.display()))?;
    }
    copy(
        &built,
        &vst3.join("Contents").join(vst3_arch_dir()).join(vst3_bin),
    )?;
    let clap = dist.join(format!("{NAME}.clap"));
    copy(&built, &clap)?;
    println!("bundled:\n  {}\n  {}", vst3.display(), clap.display());
    Ok((vst3, clap))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create {}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            copy(&entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn install(release: bool) -> Result<(), String> {
    let (vst3, clap) = bundle(release)?;
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").join("Common"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Audio/Plug-Ins"))
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    .ok_or("can't find the per-user plugin folder")?;
    let (vst3_dir, clap_dir) = if cfg!(any(windows, target_os = "macos")) {
        (base.join("VST3"), base.join("CLAP"))
    } else {
        (base.join(".vst3"), base.join(".clap"))
    };
    let vst3_dest = vst3_dir.join(format!("{NAME}.vst3"));
    if vst3_dest.exists() {
        std::fs::remove_dir_all(&vst3_dest).map_err(|e| {
            format!(
                "remove old {}: {e} (close the DAW first)",
                vst3_dest.display()
            )
        })?;
    }
    copy_dir(&vst3, &vst3_dest)?;
    copy(&clap, &clap_dir.join(format!("{NAME}.clap")))?;
    println!(
        "installed:\n  {}\n  {}",
        vst3_dest.display(),
        clap_dir.join(format!("{NAME}.clap")).display()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let release = !args.iter().any(|a| a == "--debug");
    let res = match args.first().map(String::as_str) {
        Some("bundle") => bundle(release).map(|_| ()),
        Some("install") => install(release),
        _ => Err("usage: cargo xtask <bundle|install> [--debug]".into()),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
