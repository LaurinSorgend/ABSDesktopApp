//! Where the app keeps its database, logins and covers.

use std::path::PathBuf;

const APP_DIR: &str = "ABS Desktop";
const APP_DIR_UNIX: &str = "abs-desktop";
/// Puts all data, covers and logins in one folder, for development and tests.
pub const DATA_DIR_VAR: &str = "ABS_DESKTOP_DATA_DIR";

/// The folder set by [`DATA_DIR_VAR`], if any.
pub fn override_dir() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_VAR).map(PathBuf::from)
}

/// The database and, without a keyring, the login files.
pub fn data_dir() -> PathBuf {
    override_dir().unwrap_or_else(|| platform_dir("APPDATA", "XDG_DATA_HOME", ".local/share"))
}

/// The cover cache; safe to delete.
pub fn cache_dir() -> PathBuf {
    override_dir()
        .unwrap_or_else(|| platform_dir("LOCALAPPDATA", "XDG_CACHE_HOME", ".cache"))
        .join("covers")
}

fn platform_dir(windows_var: &str, xdg_var: &str, home_fallback: &str) -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os(windows_var).map_or_else(std::env::temp_dir, PathBuf::from);
        return base.join(APP_DIR);
    }
    let base = std::env::var_os(xdg_var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(home_fallback)))
        .unwrap_or_else(std::env::temp_dir);
    base.join(APP_DIR_UNIX)
}

/// Total size of the files under `dir`.
pub fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}
