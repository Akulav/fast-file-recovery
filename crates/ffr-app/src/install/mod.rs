//! Self-install, update and uninstall. Windows uses the Inno Setup installer; Linux and macOS
//! binaries install themselves.

use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    use std::path::Path;
    pub fn startup_offer() -> super::StartupOffer {
        super::StartupOffer::None
    }
    pub fn install_and_relaunch() -> Result<(), String> {
        Err("Not supported on this system.".into())
    }
    pub fn uninstall(_delete_data: bool) -> Result<String, String> {
        Err("Not supported on this system.".into())
    }
    pub fn apply_update(_new: &Path) -> Result<(), String> {
        Err("Not supported on this system.".into())
    }
    pub fn cleanup_old() {}
}

/// What to offer when the app starts.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(windows, allow(dead_code))]
pub enum StartupOffer {
    None,
    /// Not installed: "Install for this user" / "Just run it".
    Install,
    /// An older copy is installed: "Update installed copy" / "Just run it".
    UpdateInstalled {
        installed: Option<String>,
    },
}

pub fn startup_offer() -> StartupOffer {
    imp::startup_offer()
}

/// Install this binary for the current user, then start the installed copy.
/// On success the caller should exit.
pub fn install_and_relaunch() -> Result<(), String> {
    imp::install_and_relaunch()
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    imp::uninstall(delete_data)
}

/// Apply a downloaded, verified update. On success the caller should exit.
pub fn apply_update(new: &Path) -> Result<(), String> {
    imp::apply_update(new)
}

/// Remove the `.old` binary left by the previous update.
pub fn cleanup_old() {
    imp::cleanup_old()
}

/// Version printed by `<binary> --version` ("Fast File Recovery 1.2.3").
#[allow(dead_code)]
pub(crate) fn version_of(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .last()
        .map(str::to_string)
}

/// True when `ours` is newer than `installed`.
#[allow(dead_code)]
pub(crate) fn is_newer(ours: &str, installed: Option<&str>) -> bool {
    match installed.and_then(|v| semver::Version::parse(v).ok()) {
        Some(inst) => semver::Version::parse(ours)
            .map(|o| o > inst)
            .unwrap_or(false),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer() {
        assert!(is_newer("0.2.0", Some("0.1.9")));
        assert!(!is_newer("0.1.0", Some("0.1.0")));
        assert!(is_newer("0.1.0", None));
    }
}
