//! Windows: Inno Setup (`installer/ffr.iss`) installs, updates and uninstalls, and asks for the
//! install folder once. The app only runs the setup in update mode, or the Inno uninstaller.

use super::StartupOffer;
use crate::branding::VERSION;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The app always runs from its install folder on Windows; nothing else is remembered.
pub fn remembered_root() -> Option<PathBuf> {
    None
}

pub fn before_run() -> bool {
    false
}

/// The setup writes the marker; keep its version current.
pub fn on_installed_start(root: &Path) {
    if super::installed_version(root).as_deref() != Some(VERSION) {
        let _ = std::fs::write(root.join(super::MARKER), VERSION);
    }
}

/// The setup does the installing on Windows.
pub fn startup_offer() -> StartupOffer {
    StartupOffer::None
}

pub fn install_and_relaunch(_root: &Path) -> Result<(), String> {
    Err("On Windows, install with FastFileRecovery-setup.exe.".into())
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("unknown install folder")?;
    let unins = dir.join("unins000.exe");
    if !unins.exists() {
        return Err("No installed copy found. Uninstall from Settings → Apps instead.".into());
    }
    let mut cmd = Command::new(unins);
    if delete_data {
        // The uninstaller asks about settings and saved scans itself.
        cmd.arg("/DELETEDATA");
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok("The uninstaller has started.".into())
}

/// Run the setup silently in update mode, into this copy's folder; it closes this app and
/// relaunches it.
pub fn apply_update(new: &Path) -> Result<(), String> {
    let mut cmd = Command::new(new);
    cmd.args([
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/CLOSEAPPLICATIONS",
        "/UPDATE",
    ]);
    if let Some(root) = super::program_root().filter(|r| super::is_install_root(r)) {
        cmd.arg(format!("/DIR={}", root.display()));
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the update: {e}"))
}

pub fn cleanup_old() {}
