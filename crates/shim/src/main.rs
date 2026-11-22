//! AIKD version shim: the ONLY file that lives on PATH.
//!
//! Why a shim: on Windows a running .exe is file-locked, so updating the
//! binary in place always fails while any agent session holds `serve` open.
//! The shim itself almost never changes (~100KB, std only); real versions
//! live side by side under `~/.aikd/versions/<version>/aikd.exe` plus one
//! tiny pointer file `~/.aikd/active-version`.
//!
//! Update flow (never touches a running file):
//!   1. `aikd update` writes the new binary to versions/<new>/ (fresh file)
//!   2. atomically rewrites `active-version` (rename is atomic on all OS)
//!
//! Note on versioning: `aikd --version` reports the binary's compile-time
//! `CARGO_PKG_VERSION` (e.g. `2.0.0`), while `active-version` stores the
//! management folder label (e.g. `2.0.0-identity`). Use `aikd update --list`
//! or `aikd doctor` to see both the active pointer label and binary path.
use std::path::{Path, PathBuf};

fn aikd_home() -> PathBuf {
    // Single source of truth, same as the CLI (`~/.aikd`).
    dirs_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aikd")
}

#[cfg(windows)]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(not(windows))]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn active_version(home: &Path) -> Option<String> {
    std::fs::read_to_string(home.join("active-version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "shim")
}

fn candidate(home: &Path) -> Option<PathBuf> {
    // 1. Pinned active version.
    if let Some(v) = active_version(home) {
        let p = home.join("versions").join(&v).join(exe_name());
        if p.is_file() {
            return Some(p);
        }
    }
    // 2. Newest versions/<v>/ entry (lexicographic = semver-ish enough).
    if let Ok(rd) = std::fs::read_dir(home.join("versions")) {
        let mut best: Option<PathBuf> = None;
        for e in rd.flatten() {
            let p = e.path().join(exe_name());
            if p.is_file() && best.as_ref().map(|b: &PathBuf| p > *b).unwrap_or(true) {
                best = Some(p);
            }
        }
        if best.is_some() {
            return best;
        }
    }
    // 3. Legacy in-place binary next to the home dir (pre-shim installs).
    None
}

#[cfg(windows)]
fn exe_name() -> &'static str {
    "aikd.exe"
}

#[cfg(not(windows))]
fn exe_name() -> &'static str {
    "aikd"
}

fn main() {
    let home = aikd_home();
    match candidate(&home) {
        Some(bin) => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let err = exec_replace(&bin, &args);
            eprintln!("aikd shim: failed to launch {}: {err}", bin.display());
            std::process::exit(1);
        }
        None => {
            eprintln!(
                "aikd shim: no version installed under {}. Run the installer or `aikd update` with a full binary present.",
                home.join("versions").display()
            );
            std::process::exit(1);
        }
    }
}

#[cfg(unix)]
fn exec_replace(bin: &PathBuf, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(bin).args(args).exec()
}

#[cfg(windows)]
fn exec_replace(bin: &PathBuf, args: &[String]) -> std::io::Error {
    // No exec on Windows: spawn and forward the exit code.
    match std::process::Command::new(bin).args(args).status() {
        Ok(st) => std::process::exit(st.code().unwrap_or(1)),
        Err(e) => e,
    }
}
