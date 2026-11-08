//! Windows: Inno Setup does install, update and uninstall. The app only runs the setup in
//! update mode, or the Inno uninstaller for `--uninstall`.

use super::StartupOffer;
use std::path::Path;
use std::process::Command;

pub fn startup_offer() -> StartupOffer {
    StartupOffer::None
}

pub fn install_and_relaunch() -> Result<(), String> {
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

/// Run the setup silently in update mode; it closes this app and relaunches it.
pub fn apply_update(new: &Path) -> Result<(), String> {
    Command::new(new)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/UPDATE",
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the update: {e}"))
}

pub fn cleanup_old() {}
