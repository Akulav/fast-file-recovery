//! macOS: the binary installs itself as `<folder>/Fast File Recovery.app` (ad-hoc signed) into a
//! folder the user picks (default `~/Applications/Fast File Recovery`). Settings, saved scans and
//! logs live in that same folder, next to the app.

use super::unix::{copy_self_to, forget, offer_for, pointer_root, remember, swap_in};
use super::{installed_root, same_path, StartupOffer};
use crate::branding::{APP_NAME, BIN_NAME, VERSION};
use std::path::{Path, PathBuf};
use std::process::Command;

pub use super::unix::on_installed_start;

const ICNS: &[u8] = include_bytes!("../../../../assets/icons/icon.icns");

fn applications() -> PathBuf {
    crate::paths::user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Applications")
}

/// Suggested install folder.
pub fn default_install_dir() -> PathBuf {
    applications().join(APP_NAME)
}

fn bundle(root: &Path) -> PathBuf {
    root.join(format!("{APP_NAME}.app"))
}

fn installed_bin(root: &Path) -> PathBuf {
    bundle(root).join("Contents/MacOS").join(BIN_NAME)
}

/// Versions before 0.1.3 installed the bundle straight into `~/Applications`.
fn legacy_bundle() -> PathBuf {
    bundle(&applications())
}

/// The remembered folder, or the default one when a pre-0.1.3 install exists.
pub fn remembered_root() -> Option<PathBuf> {
    pointer_root().or_else(|| legacy_bundle().is_dir().then(default_install_dir))
}

/// Running from a pre-0.1.3 bundle (after it updated itself): move into the new layout
/// (`~/Applications/Fast File Recovery/Fast File Recovery.app`) and start from there.
pub fn before_run() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let legacy = legacy_bundle();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if !legacy.is_dir() || !canon(&exe).starts_with(canon(&legacy)) {
        return false;
    }
    match install_and_relaunch(&default_install_dir()) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("could not move the app to the new layout: {e}");
            false
        }
    }
}

pub fn startup_offer() -> StartupOffer {
    offer_for(remembered_root(), installed_bin, default_install_dir())
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

fn open(app: &Path) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("-n")
        .arg(app)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open {}: {e}", app.display()))
}

/// Build `<root>/Fast File Recovery.app` around a copy of this binary, mark and remember the
/// folder, remove a pre-0.1.3 bundle, then open the app.
pub fn install_and_relaunch(root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("Couldn't create {}: {e}", root.display()))?;
    let app = bundle(root);
    let res = app.join("Contents/Resources");
    std::fs::create_dir_all(&res).map_err(|e| e.to_string())?;
    copy_self_to(&installed_bin(root))?;
    std::fs::write(res.join("icon.icns"), ICNS).map_err(|e| e.to_string())?;
    std::fs::write(app.join("Contents/Info.plist"), info_plist()).map_err(|e| e.to_string())?;
    sign(&app);
    super::write_marker(root).map_err(|e| e.to_string())?;
    remember(root);
    let legacy = legacy_bundle();
    if legacy.is_dir() && !same_path(&legacy, &app) {
        let _ = std::fs::remove_dir_all(&legacy);
    }
    open(&app)
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    let Some(root) = installed_root() else {
        return Ok("Nothing to remove; Fast File Recovery is not installed.".into());
    };
    let mut removed = Vec::new();
    if std::fs::remove_dir_all(bundle(&root)).is_ok() {
        removed.push(bundle(&root).display().to_string());
    }
    let _ = std::fs::remove_file(root.join(super::MARKER));
    forget();
    if delete_data {
        super::delete_data(&root);
        if let Some(d) = crate::paths::legacy_config_dir() {
            let _ = std::fs::remove_dir_all(d);
        }
        removed.push(format!(
            "settings, saved scans and logs in {}",
            root.display()
        ));
    }
    let _ = std::fs::remove_dir(&root);
    if removed.is_empty() {
        Ok("Nothing to remove; Fast File Recovery is not installed.".into())
    } else {
        Ok(format!("Removed:\n  {}", removed.join("\n  ")))
    }
}

/// Swap the running binary for the verified download, re-sign the bundle, relaunch.
pub fn apply_update(new: &Path) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    swap_in(new, &target)?;
    match installed_root().filter(|r| same_path(&installed_bin(r), &target)) {
        Some(root) => {
            sign(&bundle(&root));
            open(&bundle(&root))
        }
        None => Command::new(&target)
            .arg("--here")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

pub fn cleanup_old() {
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::remove_file(me.with_extension("old"));
        let _ = std::fs::remove_file(me.with_extension("new"));
    }
}
