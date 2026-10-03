//! Linux-specific paths and helpers. Everything lives under the user's own XDG directories;
//! nothing here ever needs root.
use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "linux-chess";
pub const APP_ID: &str = "io.github.catprincehq.linux_chess";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join(APP_DIR_NAME)
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join(APP_DIR_NAME)
}

/// Where engines the user imports/copies into the app live (optional; engines may stay anywhere).
pub fn user_engines_dir() -> PathBuf {
    data_dir().join("engines")
}

/// Directories that may contain engines shipped with the application package:
/// next to the executable (AppImage / tarball), under $APPDIR (AppImage), and the .deb location.
pub fn bundled_engine_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdir) = std::env::var_os("APPDIR") {
        dirs.push(PathBuf::from(appdir).join("usr/lib").join(APP_DIR_NAME).join("engines"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            dirs.push(bin.join("engines"));
            dirs.push(bin.join("../lib").join(APP_DIR_NAME).join("engines"));
        }
    }
    dirs.push(PathBuf::from("/usr/lib").join(APP_DIR_NAME).join("engines"));
    dirs
}

pub fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Finds `name` in the bundled directories, `$PATH` and Debian's `/usr/games`. Only looks: it
/// never executes anything.
pub fn locate_program(name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = bundled_engine_dirs().into_iter().map(|d| d.join(name)).collect();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(name)));
    }
    candidates.push(PathBuf::from("/usr/games").join(name));
    candidates.into_iter().find(|c| is_executable_file(c)).and_then(|c| c.canonicalize().ok())
}

/// Replace a leading `~/` with the home directory (people type that into path boxes).
pub fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}
