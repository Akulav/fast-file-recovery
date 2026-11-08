//! Admin / root handling per OS.
//!
//! Windows release builds request admin rights in the manifest. Linux uses `pkexec`, macOS uses
//! `osascript … with administrator privileges`; both pass `FFR_ORIG_UID` so the elevated copy
//! keeps using the original user's folders and gives recovered files back to that user.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ELEVATED_FLAG: OnceLock<bool> = OnceLock::new();

/// Remember whether this process was started with `--elevated`.
pub fn set_elevated_flag(v: bool) {
    let _ = ELEVATED_FLAG.set(v);
}

/// True in the copy started by "Restart with administrator rights" (Linux/macOS).
pub fn is_elevated_instance() -> bool {
    ELEVATED_FLAG.get().copied().unwrap_or(false)
}

pub fn is_elevated() -> bool {
    imp::is_elevated()
}

/// Relaunch this binary with admin rights. On success the caller should exit.
pub fn relaunch_elevated() -> Result<(), String> {
    imp::relaunch_elevated()
}

/// The original user's uid/gid when running elevated on Linux/macOS.
pub fn original_ids() -> Option<(u32, u32)> {
    imp::original_ids()
}

pub fn original_user_home() -> Option<PathBuf> {
    imp::original_user_home()
}

/// When elevated, give a file or folder back to the original user.
pub fn give_to_original_user(path: &Path) {
    if let Some((uid, gid)) = original_ids() {
        ffr_core::device::chown_tree(path, uid, gid);
    }
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn is_elevated() -> bool {
        // SAFETY: no arguments; returns a BOOL.
        unsafe { IsUserAnAdmin() != 0 }
    }

    pub fn relaunch_elevated() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let verb = wide("runas");
        let file = wide(&exe.to_string_lossy());
        let args = wide("--here");
        // SAFETY: all strings are NUL-terminated and outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                args.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if r as isize > 32 {
            Ok(())
        } else {
            Err("Windows did not start the program with administrator rights.".into())
        }
    }

    pub fn original_ids() -> Option<(u32, u32)> {
        None
    }

    pub fn original_user_home() -> Option<PathBuf> {
        None
    }
}

#[cfg(unix)]
mod imp {
    use std::ffi::CStr;
    use std::path::PathBuf;
    use std::process::Command;

    pub fn is_elevated() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    fn orig_uid() -> Option<u32> {
        if !is_elevated() {
            return None;
        }
        std::env::var("FFR_ORIG_UID").ok()?.parse().ok()
    }

    fn passwd(uid: u32) -> Option<(u32, PathBuf)> {
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        // SAFETY: zeroed passwd is a valid all-pointer/integer struct.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: all pointers are valid for the given sizes; getpwuid_r is thread-safe.
        let r =
            unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
        if r != 0 || result.is_null() || pwd.pw_dir.is_null() {
            return None;
        }
        // SAFETY: pw_dir points into `buf`, NUL-terminated by getpwuid_r.
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) }
            .to_string_lossy()
            .into_owned();
        Some((pwd.pw_gid, PathBuf::from(dir)))
    }

    pub fn original_ids() -> Option<(u32, u32)> {
        let uid = orig_uid()?;
        let (gid, _) = passwd(uid)?;
        Some((uid, gid))
    }

    pub fn original_user_home() -> Option<PathBuf> {
        passwd(orig_uid()?).map(|(_, h)| h)
    }

    #[cfg(target_os = "linux")]
    pub fn relaunch_elevated() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let mut cmd = Command::new("pkexec");
        cmd.arg("env");
        for var in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_SESSION_TYPE",
        ] {
            if let Ok(v) = std::env::var(var) {
                cmd.arg(format!("{var}={v}"));
            }
        }
        cmd.arg(format!("FFR_ORIG_UID={uid}"));
        cmd.arg(exe).arg("--elevated").arg("--here");
        cmd.spawn().map(|_| ()).map_err(|e| {
            format!("Couldn't start pkexec: {e}. Install polkit, or run the app with sudo.")
        })
    }

    #[cfg(target_os = "macos")]
    pub fn relaunch_elevated() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let quoted = exe.to_string_lossy().replace('\'', "'\\''");
        let script = format!(
            "do shell script \"FFR_ORIG_UID={uid} '{quoted}' --elevated --here > /dev/null 2>&1 &\" with administrator privileges"
        );
        Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn relaunch_elevated() -> Result<(), String> {
        Err("Not supported on this system.".into())
    }
}
