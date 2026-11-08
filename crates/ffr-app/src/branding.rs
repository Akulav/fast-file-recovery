//! Name, author, repository and version constants.

pub const APP_NAME: &str = "Fast File Recovery";
pub const MADE_BY: &str = "Made by Akulav";
pub const TAGLINE: &str =
    "Get deleted files back, fast. One program, nothing written to the disk you're saving.";
pub const REPO_URL: &str = "https://github.com/Akulav/fast-file-recovery";
pub const REPO_API_LATEST: &str =
    "https://api.github.com/repos/Akulav/fast-file-recovery/releases/latest";
pub const REPO_API_RELEASES: &str =
    "https://api.github.com/repos/Akulav/fast-file-recovery/releases?per_page=10";
pub const LICENSE: &str = "Akulav Source-Available License 1.0";
/// Matches the `.desktop` file name so Wayland shows the icon.
pub const APP_ID: &str = "fast-file-recovery";
#[cfg_attr(windows, allow(dead_code))]
pub const BIN_NAME: &str = "fast-file-recovery";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Only CI release builds set `FFR_RELEASE_BUILD=1`. Local builds never update themselves.
pub const RELEASE_BUILD: bool = match option_env!("FFR_RELEASE_BUILD") {
    Some(v) => v.len() == 1 && v.as_bytes()[0] == b'1',
    None => false,
};

/// Release asset names. These are API: installed copies look for them. Never rename.
pub const ASSET_WINDOWS: &str = "FastFileRecovery-setup.exe";
pub const ASSET_LINUX: &str = "fast-file-recovery-linux-x64";
pub const ASSET_MACOS: &str = "fast-file-recovery-macos-arm64";
pub const ASSET_SUMS: &str = "SHA256SUMS.txt";

pub fn this_os_asset() -> &'static str {
    if cfg!(windows) {
        ASSET_WINDOWS
    } else if cfg!(target_os = "macos") {
        ASSET_MACOS
    } else {
        ASSET_LINUX
    }
}

pub fn os_arch() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The 256 px PNG used for the window / taskbar icon on every OS.
pub const ICON_256: &[u8] = include_bytes!("../../../assets/icons/icon-256.png");
