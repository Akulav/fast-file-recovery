//! Linux: the binary installs itself into a folder the user picks (default
//! `~/.local/share/fast-file-recovery`), with a `~/.local/bin` symlink, a `.desktop` entry and
//! hicolor icons. Settings, saved scans and logs live in that same folder.

use super::unix::{copy_self_to, forget, offer_for, pointer_root, remember, swap_in};
use super::{installed_root, is_install_root, program_root, same_path, StartupOffer};
use crate::branding::{APP_ID, APP_NAME, BIN_NAME};
use std::path::{Path, PathBuf};
use std::process::Command;

pub use super::unix::on_installed_start;

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
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
}

/// Suggested install folder. Versions before 0.1.3 always installed here.
pub fn default_install_dir() -> PathBuf {
    data_home().join(APP_ID)
}

fn installed_bin(root: &Path) -> PathBuf {
    root.join(BIN_NAME)
}

fn symlink_path() -> PathBuf {
    home().join(".local/bin").join(BIN_NAME)
}

fn desktop_path() -> PathBuf {
    data_home()
        .join("applications")
        .join(format!("{APP_ID}.desktop"))
}

/// The remembered folder, or the pre-0.1.3 install (always in the default folder).
pub fn remembered_root() -> Option<PathBuf> {
    pointer_root().or_else(|| {
        let d = default_install_dir();
        installed_bin(&d).is_file().then_some(d)
    })
}

/// A pre-0.1.3 install updated itself in place: adopt its folder as the install folder.
pub fn before_run() -> bool {
    let Some(root) = program_root() else {
        return false;
    };
    if !is_install_root(&root) && same_path(&root, &default_install_dir()) {
        let bin = installed_bin(&root);
        if bin.is_file() && super::write_marker(&root).is_ok() {
            remember(&root);
            let _ = write_desktop_and_icons(&bin);
        }
    }
    false
}

pub fn startup_offer() -> StartupOffer {
    offer_for(remembered_root(), installed_bin, default_install_dir())
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

/// Copy this binary into `root`, mark and remember the folder, add menu entries, start it.
pub fn install_and_relaunch(root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("Couldn't create {}: {e}", root.display()))?;
    let bin = installed_bin(root);
    copy_self_to(&bin)?;
    let link = symlink_path();
    if let Some(d) = link.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::remove_file(&link);
    let _ = std::os::unix::fs::symlink(&bin, &link);
    write_desktop_and_icons(&bin)?;
    super::write_marker(root).map_err(|e| e.to_string())?;
    remember(root);
    Command::new(&bin)
        .current_dir(root)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Installed, but couldn't start it: {e}"))
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    let Some(root) = installed_root() else {
        return Ok("Nothing to remove; Fast File Recovery is not installed.".into());
    };
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
    // A running binary can be deleted on Linux; the process keeps its copy until it exits.
    let bin = installed_bin(&root);
    for p in [
        bin.clone(),
        bin.with_extension("old"),
        bin.with_extension("new"),
    ] {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_file(root.join(super::MARKER));
    removed.push(bin.display().to_string());
    forget();
    if delete_data {
        super::delete_data(&root);
        for d in [
            crate::paths::legacy_config_dir(),
            crate::paths::legacy_logs_dir(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = std::fs::remove_dir_all(d);
        }
        removed.push(format!(
            "settings, saved scans and logs in {}",
            root.display()
        ));
    }
    let _ = std::fs::remove_dir(&root);
    Ok(format!("Removed:\n  {}", removed.join("\n  ")))
}

/// Swap the running binary for the verified download (same folder, atomic rename), relaunch.
pub fn apply_update(new: &Path) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    swap_in(new, &target)?;
    Command::new(&target)
        .arg("--here")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Updated, but couldn't restart: {e}"))
}

pub fn cleanup_old() {
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::remove_file(me.with_extension("old"));
        let _ = std::fs::remove_file(me.with_extension("new"));
    }
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
