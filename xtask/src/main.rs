//! Developer tasks.
//!
//! - `cargo xtask bundle [--debug] [reference|samples]`: builds the plugins (all, or the one
//!   named) and lays out `dist/` with `<Name>.vst3` (bundle folder) and `<Name>.clap`.
//! - `cargo xtask install [--debug] [reference|samples]`: bundles, then copies them into the
//!   per-user plugin folders (`%LOCALAPPDATA%\Programs\Common\VST3` and `...\CLAP` on Windows).
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

/// A plugin the workspace ships: (short name, package, library name, display name).
#[derive(Clone, Copy)]
struct Plugin {
    short: &'static str,
    package: &'static str,
    lib: &'static str,
    name: &'static str,
}

const PLUGINS: &[Plugin] = &[
    Plugin {
        short: "reference",
        package: "solplugs-reference",
        lib: "solplugs_reference",
        name: "SolPlugs Reference",
    },
    Plugin {
        short: "samples",
        package: "solplugs-samples",
        lib: "solplugs_samples",
        name: "SolPlugs Samples",
    },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn dll_name(lib: &str) -> String {
    if cfg!(windows) {
        format!("{lib}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{lib}.dylib")
    } else {
        format!("lib{lib}.so")
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

/// Builds the plugin with only `feature` enabled and returns the built binary, copied
/// out of the target dir so the next build can't overwrite it.
fn build_format(p: Plugin, release: bool, feature: &str) -> Result<PathBuf, String> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root()).args([
        "build",
        "-p",
        p.package,
        "--no-default-features",
        "--features",
        feature,
    ]);
    if release {
        cmd.arg("--release");
    }
    let status = cmd.status().map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build ({feature}) failed"));
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let profile = target.join(if release { "release" } else { "debug" });
    let out = profile.join(format!("{feature}-{}", dll_name(p.lib)));
    copy(&profile.join(dll_name(p.lib)), &out)?;
    Ok(out)
}

fn bundle(p: Plugin, release: bool) -> Result<(PathBuf, PathBuf), String> {
    let name = p.name;
    let vst3_built = build_format(p, release, "vst3")?;
    let clap_built = build_format(p, release, "clap")?;
    let dist = root().join("dist");
    let ext = if cfg!(windows) {
        "vst3"
    } else if cfg!(target_os = "macos") {
        ""
    } else {
        "so"
    };
    let vst3_bin = if cfg!(target_os = "macos") {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    };
    let vst3 = dist.join(format!("{name}.vst3"));
    if vst3.exists() {
        std::fs::remove_dir_all(&vst3).map_err(|e| format!("clean {}: {e}", vst3.display()))?;
    }
    copy(
        &vst3_built,
        &vst3.join("Contents").join(vst3_arch_dir()).join(vst3_bin),
    )?;
    let clap = dist.join(format!("{name}.clap"));
    copy(&clap_built, &clap)?;
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

fn install(p: Plugin, release: bool) -> Result<(), String> {
    let name = p.name;
    let (vst3, clap) = bundle(p, release)?;
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
    let vst3_dest = vst3_dir.join(format!("{name}.vst3"));
    if vst3_dest.exists() {
        std::fs::remove_dir_all(&vst3_dest).map_err(|e| {
            format!(
                "remove old {}: {e} (close the DAW first)",
                vst3_dest.display()
            )
        })?;
    }
    copy_dir(&vst3, &vst3_dest)?;
    copy(&clap, &clap_dir.join(format!("{name}.clap")))?;
    println!(
        "installed:\n  {}\n  {}",
        vst3_dest.display(),
        clap_dir.join(format!("{name}.clap")).display()
    );
    Ok(())
}

mod probe;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let release = !args.iter().any(|a| a == "--debug");
    let chosen: Vec<Plugin> = match args.iter().skip(1).find(|a| !a.starts_with("--")) {
        Some(short) => PLUGINS
            .iter()
            .copied()
            .filter(|p| p.short == short)
            .collect(),
        None => PLUGINS.to_vec(),
    };
    let each = |f: fn(Plugin, bool) -> Result<(), String>| -> Result<(), String> {
        if chosen.is_empty() {
            return Err("unknown plugin (use reference or samples)".into());
        }
        chosen.iter().try_for_each(|p| f(*p, release))
    };
    let res = match args.first().map(String::as_str) {
        Some("bundle") => each(|p, r| bundle(p, r).map(|_| ())),
        Some("install") => each(install),
        Some("probe-vst3") => match args.get(1) {
            Some(path) => probe::run(std::path::Path::new(path)),
            None => Err("usage: cargo xtask probe-vst3 <path to .vst3 binary>".into()),
        },
        _ => Err(
            "usage: cargo xtask <bundle|install> [reference|samples] [--debug] | probe-vst3 PATH"
                .into(),
        ),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
