//! Settings, sessions and logs locations. When elevated on Linux/macOS these resolve against
//! the original user's home (`FFR_ORIG_UID`), never root's.

use std::path::PathBuf;

/// Home directory of the user who started the app (before elevation).
pub fn user_home() -> Option<PathBuf> {
    if let Some(home) = crate::elevate::original_user_home() {
        return Some(home);
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().unwrap_or_default().join("AppData/Roaming"))
            .join("FastFileRecovery")
    } else if cfg!(target_os = "macos") {
        user_home()
            .unwrap_or_default()
            .join("Library/Application Support/FastFileRecovery")
    } else {
        let base = if crate::elevate::original_user_home().is_some() {
            None
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        };
        base.unwrap_or_else(|| user_home().unwrap_or_default().join(".config"))
            .join("fast-file-recovery")
    }
}

pub fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}

pub fn logs_dir() -> PathBuf {
    if cfg!(windows) || cfg!(target_os = "macos") {
        config_dir().join("logs")
    } else {
        let base = if crate::elevate::original_user_home().is_some() {
            None
        } else {
            std::env::var_os("XDG_STATE_HOME").map(PathBuf::from)
        };
        base.unwrap_or_else(|| user_home().unwrap_or_default().join(".local/state"))
            .join("fast-file-recovery")
    }
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.toml")
}
