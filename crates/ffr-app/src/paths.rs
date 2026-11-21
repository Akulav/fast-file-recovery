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

/// Linux/macOS: folder for the one-line `install-dir` pointer, the only file kept outside the
/// install folder (so a newer download can find and update the installed copy).
#[cfg(unix)]
pub fn pointer_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return Some(user_home()?.join("Library/Application Support/FastFileRecovery"));
    }
    // `$XDG_CONFIG_HOME` unless elevated (root's environment is not the user's).
    let from_env = if crate::elevate::original_user_home().is_some() {
        None
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = from_env.or_else(|| user_home().map(|h| h.join(".config")))?;
    Some(base.join("fast-file-recovery"))
}
