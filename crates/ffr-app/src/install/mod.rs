//! Where the app lives: installing, updating and removing it.
//!
//! **One file per OS does both install and update.**
//!
//! * **Windows** — `FastFileRecovery-setup.exe` (Inno Setup, `installer/ffr.iss`). The first run
//!   asks for the install folder. Inno remembers it, so running a newer setup later, or the in-app
//!   update (which starts the setup with `/UPDATE`), installs into the same folder without asking.
//! * **Linux / macOS** — the bare binary. Started from anywhere, it offers to install itself into a
//!   folder the user picks; the folder is remembered in `<old config dir>/install-dir`. A newer
//!   download started later offers to update that folder. In-app updates swap the binary in place.
//!
//! **Files go there.** The install folder holds the program *and* everything the app writes
//! (settings, saved scans, logs, downloaded updates). A [`MARKER`] file identifies an install
//! folder. Data left in the pre-0.1.3 locations (`%APPDATA%\FastFileRecovery`,
//! `~/.config/fast-file-recovery`, …) is moved in on the first start of the installed copy.

use std::path::{Path, PathBuf};

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
    use std::path::{Path, PathBuf};
    pub fn remembered_root() -> Option<PathBuf> {
        None
    }
    pub fn before_run() -> bool {
        false
    }
    pub fn on_installed_start(_root: &Path) {}
    pub fn startup_offer() -> super::StartupOffer {
        super::StartupOffer::None
    }
    pub fn install_and_relaunch(_root: &Path) -> Result<(), String> {
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

/// Present in every install folder; holds the installed version.
pub const MARKER: &str = ".ffr-install";
/// Everything the app keeps in its folder. Deleting data removes these and nothing else.
pub const DATA_ENTRIES: &[&str] = &["sessions", "logs", "update", "settings.toml"];

// ---------------------------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------------------------

/// Folder a program belongs to: its own folder, or for `X.app/Contents/MacOS/bin` the folder that
/// holds `X.app`.
pub fn root_of_exe(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let bundle = dir
        .parent()
        .filter(|_| dir.file_name().is_some_and(|n| n == "MacOS"))
        .filter(|contents| contents.file_name().is_some_and(|n| n == "Contents"))
        .and_then(Path::parent)
        .filter(|b| b.extension().is_some_and(|e| e == "app"));
    match bundle {
        Some(b) => b.parent().map(Path::to_path_buf),
        None => Some(dir.to_path_buf()),
    }
}

/// Folder of the running program (see [`root_of_exe`]).
pub fn program_root() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| root_of_exe(&e))
}

pub fn is_install_root(dir: &Path) -> bool {
    dir.join(MARKER).is_file()
}

/// Version recorded in an install folder's marker.
pub fn installed_version(root: &Path) -> Option<String> {
    let v = std::fs::read_to_string(root.join(MARKER)).ok()?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn write_marker(root: &Path) -> std::io::Result<()> {
    std::fs::write(root.join(MARKER), crate::branding::VERSION)
}

/// The install folder: the running copy's, otherwise the remembered one.
pub fn installed_root() -> Option<PathBuf> {
    program_root()
        .filter(|r| is_install_root(r))
        .or_else(|| imp::remembered_root().filter(|r| is_install_root(r)))
}

/// `portable.txt` next to the program, `--portable` or `FFR_PORTABLE`: keep data next to it and
/// never offer to install.
pub fn portable() -> bool {
    std::env::var_os("FFR_PORTABLE").is_some()
        || program_root().is_some_and(|r| r.join("portable.txt").exists())
}

/// The app's data folder: always the folder it lives in — the install folder when installed,
/// otherwise next to the program.
pub fn data_root() -> Option<PathBuf> {
    if portable() {
        return program_root();
    }
    installed_root().or_else(program_root)
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (canon(a), canon(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Check a folder typed into the install dialog. `~` expands to the home folder.
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_install_dir(text: &str) -> Result<PathBuf, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Choose a folder.".into());
    }
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => crate::paths::user_home()
            .ok_or("Couldn't find your home folder.")?
            .join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    };
    if !path.is_absolute() {
        return Err("Use a full path, for example ~/Apps/FastFileRecovery.".into());
    }
    if path.parent().is_none() {
        return Err("Pick a folder, not the root of a drive.".into());
    }
    if path.is_file() {
        return Err("That's a file, not a folder.".into());
    }
    Ok(path)
}

// ---------------------------------------------------------------------------------------------
// Moving old data into the install folder
// ---------------------------------------------------------------------------------------------

fn remove_any(p: &Path) -> std::io::Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// Copy a file or a whole folder tree.
fn copy_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_all(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Move one file or folder. A rename is instant; across drives (e.g. `C:` → `D:`) it is copied
/// and the original deleted. A failed copy leaves the original untouched.
fn move_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if let Err(e) = copy_all(src, dst) {
        let _ = remove_any(dst);
        return Err(e);
    }
    let _ = remove_any(src);
    Ok(())
}

/// Move [`DATA_ENTRIES`] from `from` to `to`. If anything fails, everything moved so far goes
/// back. Entries already present in `to` are left. Returns how many entries moved.
fn move_data(from: &Path, to: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut moved: Vec<&str> = Vec::new();
    for name in DATA_ENTRIES {
        let (src, dst) = (from.join(name), to.join(name));
        if !src.exists() || dst.exists() {
            continue;
        }
        if let Err(e) = move_entry(&src, &dst) {
            for n in moved.iter().rev() {
                let _ = move_entry(&to.join(n), &from.join(n));
            }
            return Err(format!("couldn't move {}: {e}", src.display()));
        }
        moved.push(name);
    }
    let _ = std::fs::remove_dir(from); // only succeeds when nothing else is left
    Ok(moved.len())
}

/// Bring settings, saved scans and logs from the pre-0.1.3 locations into the install folder.
fn migrate_legacy_data(root: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(old) = crate::paths::legacy_config_dir().filter(|o| !same_path(o, root)) {
        match move_data(&old, root) {
            Ok(0) => {}
            Ok(_) => notes.push(format!(
                "Moved settings and saved scans from {} to {}",
                old.display(),
                root.display()
            )),
            Err(e) => notes.push(format!("Old settings were left in {} ({e})", old.display())),
        }
    }
    if let Some(old_logs) = crate::paths::legacy_logs_dir() {
        let new_logs = root.join("logs");
        if old_logs.is_dir() && !new_logs.exists() && !same_path(&old_logs, &new_logs) {
            let _ = move_entry(&old_logs, &new_logs);
        }
    }
    notes
}

// ---------------------------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------------------------

/// What to offer when the app starts (Linux/macOS, started outside the install folder).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(windows, allow(dead_code))]
pub enum StartupOffer {
    None,
    /// Not installed: pick a folder ("Install" / "Just run it").
    Install {
        suggested: PathBuf,
    },
    /// An older copy is installed in `root`: "Update installed copy" / "Just run it".
    UpdateInstalled {
        root: PathBuf,
        installed: Option<String>,
    },
}

pub struct Startup {
    pub offer: StartupOffer,
    /// Things worth a log line (e.g. data moved).
    pub notes: Vec<String>,
    /// The work was handed to another copy; exit right away.
    pub exit: bool,
}

/// Runs before logging starts (so old logs can still be moved). `may_install` is false in the
/// elevated copy: it never installs, updates or moves anything.
pub fn on_startup(here: bool, may_install: bool) -> Startup {
    let mut s = Startup {
        offer: StartupOffer::None,
        notes: Vec::new(),
        exit: false,
    };
    if !may_install {
        return s;
    }
    imp::cleanup_old();
    // A finished (or abandoned) update download.
    let _ = std::fs::remove_dir_all(crate::paths::update_dir());
    if imp::before_run() {
        s.exit = true;
        return s;
    }
    if let Some(root) = program_root().filter(|r| is_install_root(r)) {
        imp::on_installed_start(&root);
        s.notes = migrate_legacy_data(&root);
    } else if !cfg!(debug_assertions) && !here && !portable() {
        s.offer = imp::startup_offer();
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Install, uninstall, update
// ---------------------------------------------------------------------------------------------

/// Install this binary into `root` (or update the copy there), then start it.
/// On success the caller should exit. Windows installs with the setup instead.
pub fn install_and_relaunch(root: &Path) -> Result<(), String> {
    imp::install_and_relaunch(root)
}

pub fn uninstall(delete_data: bool) -> Result<String, String> {
    imp::uninstall(delete_data)
}

/// Apply a downloaded, verified update. On success the caller should exit.
pub fn apply_update(new: &Path) -> Result<(), String> {
    imp::apply_update(new)
}

/// Delete [`DATA_ENTRIES`] in `dir` (never anything else), then `dir` itself if it is empty.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn delete_data(dir: &Path) {
    for name in DATA_ENTRIES {
        let _ = remove_any(&dir.join(name));
    }
    let _ = std::fs::remove_dir(dir);
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

// ---------------------------------------------------------------------------------------------
// Shared by Linux and macOS
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
pub(crate) mod unix {
    use super::{installed_version, is_newer, version_of, StartupOffer};
    use crate::branding::VERSION;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// One line holding the install folder, so a newer download can offer to update it.
    fn pointer_file() -> Option<PathBuf> {
        crate::paths::legacy_config_dir().map(|d| d.join("install-dir"))
    }

    pub fn pointer_root() -> Option<PathBuf> {
        let text = std::fs::read_to_string(pointer_file()?).ok()?;
        let dir = text.trim();
        (!dir.is_empty()).then(|| PathBuf::from(dir))
    }

    pub fn remember(root: &Path) {
        if let Some(file) = pointer_file() {
            if let Some(d) = file.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&file, root.to_string_lossy().as_bytes());
        }
    }

    pub fn forget() {
        if let Some(f) = pointer_file() {
            let _ = std::fs::remove_file(&f);
            if let Some(d) = f.parent() {
                let _ = std::fs::remove_dir(d);
            }
        }
    }

    /// Keep the marker's version current after in-app updates, and the folder remembered.
    pub fn on_installed_start(root: &Path) {
        if installed_version(root).as_deref() != Some(VERSION) {
            let _ = super::write_marker(root);
        }
        if pointer_root().is_none_or(|r| !super::same_path(&r, root)) {
            remember(root);
        }
    }

    /// `root` holds an installed copy at `bin`: offer to update it when this file is newer.
    pub fn offer_for(
        root: Option<PathBuf>,
        bin: impl Fn(&Path) -> PathBuf,
        suggested: PathBuf,
    ) -> StartupOffer {
        let Some(root) = root else {
            return StartupOffer::Install { suggested };
        };
        let bin = bin(&root);
        if !bin.is_file() {
            return StartupOffer::UpdateInstalled {
                root,
                installed: None,
            };
        }
        let installed = installed_version(&root).or_else(|| version_of(&bin));
        if is_newer(VERSION, installed.as_deref()) {
            StartupOffer::UpdateInstalled { root, installed }
        } else {
            StartupOffer::None
        }
    }

    /// Copy this program to `dest` (via `dest.new`, then a rename, so a running copy is never
    /// half-written).
    pub fn copy_self_to(dest: &Path) -> Result<(), String> {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        if super::same_path(&me, dest) {
            return Ok(());
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        swap_in(&me, dest)
    }

    /// Replace `target` with a copy of `source`; the previous file stays as `.old` until the next
    /// start (renaming a running program is fine on Linux and macOS).
    pub fn swap_in(source: &Path, target: &Path) -> Result<(), String> {
        let staged = target.with_extension("new");
        std::fs::copy(source, &staged).map_err(|e| format!("Couldn't copy the program: {e}"))?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        let old = target.with_extension("old");
        let _ = std::fs::remove_file(&old);
        let had_target = target.exists();
        if had_target {
            std::fs::rename(target, &old).map_err(|e| e.to_string())?;
        }
        if let Err(e) = std::fs::rename(&staged, target) {
            if had_target {
                let _ = std::fs::rename(&old, target);
            }
            return Err(e.to_string());
        }
        Ok(())
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

    #[test]
    fn program_roots() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(
            root_of_exe(&p("/opt/ffr/fast-file-recovery")),
            Some(p("/opt/ffr"))
        );
        assert_eq!(
            root_of_exe(&p(
                "/Users/a/Applications/Fast File Recovery/Fast File Recovery.app/Contents/MacOS/fast-file-recovery"
            )),
            Some(p("/Users/a/Applications/Fast File Recovery"))
        );
        assert_eq!(
            root_of_exe(&p("/x/MacOS/fast-file-recovery")),
            Some(p("/x/MacOS"))
        );
    }

    #[test]
    fn moves_data_across_folders_and_skips_existing() {
        let d = std::env::temp_dir().join(format!("ffr-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (old, new) = (d.join("old"), d.join("new"));
        std::fs::create_dir_all(old.join("sessions/x")).unwrap();
        std::fs::write(old.join("settings.toml"), b"v").unwrap();
        std::fs::write(old.join("unrelated.txt"), b"keep").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("settings.toml"), b"newer").unwrap();
        assert_eq!(move_data(&old, &new).unwrap(), 1);
        assert!(new.join("sessions/x").is_dir());
        assert_eq!(std::fs::read(new.join("settings.toml")).unwrap(), b"newer");
        assert!(old.join("unrelated.txt").is_file(), "only app data moves");

        copy_all(&new.join("sessions"), &d.join("copy")).unwrap();
        assert!(d.join("copy/x").is_dir());
        move_entry(&d.join("copy"), &d.join("moved")).unwrap();
        assert!(d.join("moved/x").is_dir() && !d.join("copy").exists());

        delete_data(&new);
        assert!(!new.join("sessions").exists() && !new.join("settings.toml").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn install_dir_input() {
        assert!(parse_install_dir("").is_err());
        assert!(parse_install_dir("relative/dir").is_err());
        if cfg!(unix) {
            assert!(parse_install_dir("/").is_err());
            assert_eq!(
                parse_install_dir(" /opt/ffr ").unwrap(),
                PathBuf::from("/opt/ffr")
            );
        } else {
            assert!(parse_install_dir(r"C:\").is_err());
            assert_eq!(
                parse_install_dir(r"D:\Apps\FFR").unwrap(),
                PathBuf::from(r"D:\Apps\FFR")
            );
        }
    }
}
