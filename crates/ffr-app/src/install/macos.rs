//! macOS self-install into `~/Applications/Fast File Recovery.app` (ad-hoc signed).

use super::{is_newer, version_of, StartupOffer};
use crate::branding::{APP_NAME, BIN_NAME, VERSION};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICNS: &[u8] = include_bytes!("../../../../assets/icons/icon.icns");

fn app_dir() -> PathBuf {
    crate::paths::user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Applications")
        .join(format!("{APP_NAME}.app"))
}

fn bundle_bin() -> PathBuf {
    app_dir().join("Contents/MacOS").join(BIN_NAME)
}

fn running_in_bundle() -> bool {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

pub fn startup_offer() -> StartupOffer {
    if running_in_bundle() {
        return StartupOffer::None;
    }
    let bin = bundle_bin();
    if !bin.exists() {
        return StartupOffer::Install;
    }
    let installed = version_of(&bin);
    if is_newer(VERSION, installed.as_deref()) {
        StartupOffer::UpdateInstalled { installed }
    } else {
        StartupOffer::None
    }
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>{APP_NAME}</string>
  <key>CFBundleDisplayName</key><string>{APP_NAME}</string>
  <key>CFBundleIdentifier</key><string>com.akulav.fastfilerecovery</string>
  <key>CFBundleExecutable</key><string>{BIN_NAME}</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{VERSION}</string>
  <key>CFBundleVersion</key><string>{VERSION}</string>
  <key>NSHumanReadableCopyright</key><string>Made by Akulav</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

fn sign(path: &Path) {
    let _ = Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "-s", "-"])
        .arg(path)
        .output();
}

pub fn install_and_relaunch() -> Result<(), String> {
    let app = app_dir();
    let macos = app.join("Contents/MacOS");
    let res = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&res).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin = bundle_bin();
    let tmp = bin.with_extension("new");
    std::fs::copy(&me, &tmp).map_err(|e| format!("Couldn't copy the program: {e}"))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &bin).map_err(|e| e.to_string())?;
    std::fs::write(res.join("icon.icns"), ICNS).map_err(|e| e.to_string())?;
    std::fs::write(app.join("Contents/Info.plist"), info_plist()).map_err(|e| e.to_string())?;
    sign(&app);
    Command::new("/usr/bin/open")
        .arg(&app)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Installed, but couldn't open it: {e}"))
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    let mut removed = Vec::new();
    if std::fs::remove_dir_all(app_dir()).is_ok() {
        removed.push(app_dir().display().to_string());
    }
    if delete_data {
        let d = crate::paths::config_dir();
        if std::fs::remove_dir_all(&d).is_ok() {
            removed.push(d.display().to_string());
        }
    }
    if removed.is_empty() {
        Ok("Nothing to remove; Fast File Recovery is not installed.".into())
    } else {
        Ok(format!("Removed:\n  {}", removed.join("\n  ")))
    }
}

pub fn apply_update(new: &Path) -> Result<(), String> {
    let target = if bundle_bin().exists() {
        bundle_bin()
    } else {
        std::env::current_exe().map_err(|e| e.to_string())?
    };
    let staged = target.with_extension("new");
    std::fs::copy(new, &staged).map_err(|e| format!("Couldn't stage the update: {e}"))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    let old = target.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&target, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&staged, &target) {
        let _ = std::fs::rename(&old, &target);
        return Err(e.to_string());
    }
    if target == bundle_bin() {
        sign(&app_dir());
        Command::new("/usr/bin/open")
            .arg("-n")
            .arg(app_dir())
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        Command::new(&target)
            .arg("--here")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub fn cleanup_old() {
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::remove_file(me.with_extension("old"));
    }
}
