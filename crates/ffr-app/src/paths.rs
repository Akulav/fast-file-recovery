//! Where the app keeps its files: **always the folder it is installed in** (see
//! [`crate::install::data_root`]), never `%APPDATA%`, `~/.config` or other system folders.
//!
//! ```text
//! <install folder>/
//!   fast-file-recovery(.exe)   the program (macOS: Fast File Recovery.app)
//!   settings.toml              settings
//!   sessions/                  saved and resumable scans
//!   logs/                      daily log files (7 kept)
//!   update/                    a downloaded update while it is being applied
//!   .ffr-install               marks the install folder (holds the installed version)
//! ```
//!
//! `FFR_DATA_DIR` overrides the folder (testing). The elevated copy on Linux/macOS runs from the
//! same folder, so it uses the same files; it hands them back to the original user.

use std::path::PathBuf;

/// Home directory of the user who started the app (before elevation).
pub fn user_home() -> Option<PathBuf> {
    if let Some(home) = crate::elevate::original_user_home() {
        return Some(home);
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The data folder: the install folder (or next to the program when not installed).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("FFR_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    crate::install::data_root().unwrap_or_else(|| PathBuf::from("."))
}

pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.toml")
}

/// Downloaded updates are staged here, inside the install folder (never the shared temp folder).
pub fn update_dir() -> PathBuf {
    data_dir().join("update")
}

/// Where versions before 0.1.3 kept settings and sessions (moved into the install folder on the
/// first start of an installed copy).
pub fn legacy_config_dir() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| user_home().map(|h| h.join("AppData/Roaming")))?
            .join("FastFileRecovery")
    } else if cfg!(target_os = "macos") {
        user_home()?.join("Library/Application Support/FastFileRecovery")
    } else {
        xdg("XDG_CONFIG_HOME", ".config")?.join("fast-file-recovery")
    };
    Some(dir)
}

/// An XDG base folder (`$XDG_…` unless elevated, else `~/<fallback>`).
fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    let from_env = if crate::elevate::original_user_home().is_some() {
        None
    } else {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    from_env.or_else(|| user_home().map(|h| h.join(fallback)))
}

/// Where versions before 0.1.3 kept logs on Linux (Windows/macOS kept them in the config folder).
pub fn legacy_logs_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        xdg("XDG_STATE_HOME", ".local/state").map(|d| d.join("fast-file-recovery"))
    } else {
        legacy_config_dir().map(|d| d.join("logs"))
    }
}
