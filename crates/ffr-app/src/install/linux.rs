//! Linux self-install into `~/.local/share/fast-file-recovery` with a `~/.local/bin` symlink,
//! a `.desktop` entry and hicolor icons.

use super::{is_newer, version_of, StartupOffer};
use crate::branding::{APP_ID, APP_NAME, BIN_NAME, VERSION};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICONS: &[(u32, &[u8])] = &[
    (16, include_bytes!("../../../../assets/icons/icon-16.png")),
    (24, include_bytes!("../../../../assets/icons/icon-24.png")),
    (32, include_bytes!("../../../../assets/icons/icon-32.png")),
    (48, include_bytes!("../../../../assets/icons/icon-48.png")),
    (64, include_bytes!("../../../../assets/icons/icon-64.png")),
    (128, include_bytes!("../../../../assets/icons/icon-128.png")),
    (256, include_bytes!("../../../../assets/icons/icon-256.png")),
    (512, include_bytes!("../../../../assets/icons/icon-512.png")),
];

fn home() -> PathBuf {
    crate::paths::user_home().unwrap_or_else(|| PathBuf::from("."))
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
}

fn install_dir() -> PathBuf {
    data_home().join(APP_ID)
}

fn installed_bin() -> PathBuf {
    install_dir().join(BIN_NAME)
}

fn symlink_path() -> PathBuf {
    home().join(".local/bin").join(BIN_NAME)
}

fn desktop_path() -> PathBuf {
    data_home()
        .join("applications")
        .join(format!("{APP_ID}.desktop"))
}

fn running_installed() -> bool {
    let me = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    let inst = std::fs::canonicalize(installed_bin()).ok();
    me.is_some() && me == inst
}

pub fn startup_offer() -> StartupOffer {
    if running_installed() {
        return StartupOffer::None;
    }
    let bin = installed_bin();
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

fn copy_self_to(dest: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("new");
    std::fs::copy(&me, &tmp).map_err(|e| format!("Couldn't copy the program: {e}"))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Quote a path for a `.desktop` `Exec=` key (Desktop Entry spec: quoted argument rules,
/// then the string-value escaping of `\`).
fn desktop_exec_arg(path: &str) -> String {
    let mut q = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q.replace('\\', "\\\\").replace('%', "%%")
}

fn write_desktop_and_icons(bin: &Path) -> Result<(), String> {
    let desktop = desktop_path();
    if let Some(d) = desktop.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Recover deleted files — Made by Akulav\nExec={}\nIcon={APP_ID}\nTerminal=false\nCategories=Utility;System;\nStartupWMClass={APP_ID}\nKeywords=recovery;undelete;restore;disk;\n",
        desktop_exec_arg(&bin.to_string_lossy())
    );
    std::fs::write(&desktop, entry).map_err(|e| e.to_string())?;
    for (size, png) in ICONS {
        let dir = data_home()
            .join("icons/hicolor")
            .join(format!("{size}x{size}"))
            .join("apps");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{APP_ID}.png")), png).map_err(|e| e.to_string())?;
    }
    let _ = Command::new("update-desktop-database")
        .arg(data_home().join("applications"))
        .output();
    let _ = Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(data_home().join("icons/hicolor"))
        .output();
    Ok(())
}

pub fn install_and_relaunch() -> Result<(), String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bin = installed_bin();
    copy_self_to(&bin)?;
    let link = symlink_path();
    if let Some(d) = link.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::remove_file(&link);
    let _ = std::os::unix::fs::symlink(&bin, &link);
    write_desktop_and_icons(&bin)?;
    Command::new(&bin)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Installed, but couldn't start it: {e}"))
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    let mut removed = Vec::new();
    for p in [symlink_path(), desktop_path()] {
        if std::fs::remove_file(&p).is_ok() {
            removed.push(p.display().to_string());
        }
    }
    for (size, _) in ICONS {
        let _ = std::fs::remove_file(
            data_home()
                .join("icons/hicolor")
                .join(format!("{size}x{size}/apps/{APP_ID}.png")),
        );
    }
    if std::fs::remove_dir_all(install_dir()).is_ok() {
        removed.push(install_dir().display().to_string());
    }
    if delete_data {
        for d in [crate::paths::config_dir(), crate::paths::logs_dir()] {
            if std::fs::remove_dir_all(&d).is_ok() {
                removed.push(d.display().to_string());
            }
        }
    }
    if removed.is_empty() {
        Ok("Nothing to remove; Fast File Recovery is not installed.".into())
    } else {
        Ok(format!("Removed:\n  {}", removed.join("\n  ")))
    }
}

/// Swap the binary atomically (same filesystem), relaunch.
pub fn apply_update(new: &Path) -> Result<(), String> {
    let target = if installed_bin().exists() {
        installed_bin()
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
    Command::new(&target)
        .arg("--here")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn cleanup_old() {
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::remove_file(me.with_extension("old"));
    }
    let _ = std::fs::remove_file(installed_bin().with_extension("old"));
}

#[cfg(test)]
mod tests {
    use super::desktop_exec_arg;

    #[test]
    fn exec_quoting() {
        assert_eq!(desktop_exec_arg("/home/a/bin"), "\"/home/a/bin\"");
        assert_eq!(desktop_exec_arg("/x/$y"), "\"/x/\\\\$y\"");
        assert_eq!(desktop_exec_arg("/x/50%"), "\"/x/50%%\"");
    }
}
