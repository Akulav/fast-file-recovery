//! Updater: check GitHub releases, download this OS's asset, verify SHA-256, apply.
//! Never runs during a scan or recovery, never as root on Linux/macOS, never in local builds,
//! and never installs anything without the user clicking Update.

use crate::branding::{self, ASSET_SUMS, REPO_API_LATEST, REPO_API_RELEASES, VERSION};
use crossbeam_channel::{unbounded, Receiver, Sender};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub sums_url: String,
}

#[derive(Clone, Debug)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        done: u64,
        total: u64,
    },
    Verifying,
    /// Downloaded and verified; the app should exit after starting the apply step.
    Applying,
    Error(String),
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Progress(u64, u64),
    Verifying,
    Ready(Result<PathBuf, String>),
}

pub struct Updater {
    pub state: UpdateState,
    rx: Option<Receiver<Msg>>,
    started: Instant,
    last_check: Option<Instant>,
    /// Downloaded, verified file waiting to be applied.
    pub ready: Option<PathBuf>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

fn user_agent() -> String {
    format!("fast-file-recovery/{VERSION}")
}

fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    ureq::get(url)
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub ({e}). Check your connection and try again."))
}

fn current() -> Version {
    Version::parse(VERSION).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// Release downloads must come from GitHub over HTTPS.
fn trusted_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
        || url.starts_with("https://objects.githubusercontent.com/")
}

/// No release binary is anywhere near this; refuse anything bigger.
const MAX_ASSET: u64 = 512 << 20;

fn to_release(r: GhRelease) -> Option<Release> {
    let v = Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
    let asset = r
        .assets
        .iter()
        .find(|a| a.name == branding::this_os_asset())?;
    let sums = r.assets.iter().find(|a| a.name == ASSET_SUMS)?;
    if !trusted_url(&asset.browser_download_url) || !trusted_url(&sums.browser_download_url) {
        return None;
    }
    Some(Release {
        version: v,
        notes: r.body.unwrap_or_default(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size,
        sums_url: sums.browser_download_url.clone(),
    })
}

fn check() -> Result<Option<Release>, String> {
    let cur = current();
    let best = if cur.pre.is_empty() {
        let mut resp = get(REPO_API_LATEST)?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        let r: GhRelease = serde_json::from_str(&text)
            .map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
        to_release(r)
    } else {
        // Pre-release builds also look at pre-releases.
        let mut resp = get(REPO_API_RELEASES)?;
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        let list: Vec<GhRelease> = serde_json::from_str(&text)
            .map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
        list.into_iter()
            .filter(|r| !r.draft)
            .filter_map(to_release)
            .max_by(|a, b| a.version.cmp(&b.version))
    };
    Ok(best.filter(|r| r.version > cur))
}

/// Parse `SHA256SUMS.txt` (`<hex>  <name>` or `<hex> *<name>`).
pub fn find_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// A fresh folder only this user can use. A fixed name in a shared temp folder would let
/// another local user plant or swap the file between the checksum check and the install.
fn private_dir(version: &Version) -> Result<PathBuf, String> {
    let base = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for n in 0..100u32 {
        let dir = base.join(format!(
            "ffr-update-{version}-{}-{nanos:x}-{n}",
            std::process::id()
        ));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        match b.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Couldn't create a download folder: {e}")),
        }
    }
    Err("Couldn't create a download folder.".into())
}

fn download(rel: &Release, tx: &Sender<Msg>) -> Result<PathBuf, String> {
    if !trusted_url(&rel.asset_url) || !trusted_url(&rel.sums_url) {
        return Err("The update link does not point to GitHub, so it was not used.".into());
    }
    let dir = private_dir(&rel.version)?;
    let path = dir.join(branding::this_os_asset());
    let resp = get(&rel.asset_url)?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(rel.asset_size);
    let mut reader = resp.into_body().into_reader();
    if total > MAX_ASSET {
        return Err("The update is unexpectedly large, so it was not downloaded.".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > MAX_ASSET {
            drop(file);
            let _ = std::fs::remove_dir_all(&dir);
            return Err("The update is unexpectedly large, so it was deleted.".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        if last.elapsed() > Duration::from_millis(100) {
            last = Instant::now();
            let _ = tx.send(Msg::Progress(done, total));
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    let _ = tx.send(Msg::Verifying);
    let mut sums_resp = get(&rel.sums_url)?;
    let sums = sums_resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let want = find_sum(&sums, branding::this_os_asset())
        .ok_or("The release has no checksum for this file, so it was not installed.")?;
    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != want {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(
            "The download did not match its SHA-256 checksum, so it was deleted. Try again later."
                .into(),
        );
    }
    Ok(path)
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            state: UpdateState::Idle,
            rx: None,
            started: Instant::now(),
            last_check: None,
            ready: None,
        }
    }
}

impl Updater {
    /// Updates are possible only in CI release builds, in the user's non-elevated instance.
    pub fn enabled() -> bool {
        branding::RELEASE_BUILD && !crate::elevate::is_elevated_instance()
    }

    pub fn disabled_reason() -> Option<&'static str> {
        if !branding::RELEASE_BUILD {
            Some("Development build: updates disabled")
        } else if crate::elevate::is_elevated_instance() {
            Some("Updates run in the normal (non-administrator) window")
        } else {
            None
        }
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Checking
                | UpdateState::Downloading { .. }
                | UpdateState::Verifying
                | UpdateState::Applying
        )
    }

    pub fn available(&self) -> Option<&Release> {
        match &self.state {
            UpdateState::Available(r) => Some(r),
            _ => None,
        }
    }

    /// Automatic checks: 20 s after start, then every 6 h.
    pub fn tick(&mut self, auto: bool) {
        if !auto || !Self::enabled() || self.busy() {
            return;
        }
        let due = match self.last_check {
            None => self.started.elapsed() >= Duration::from_secs(20),
            Some(t) => t.elapsed() >= Duration::from_secs(6 * 3600),
        };
        if due && !matches!(self.state, UpdateState::Available(_)) {
            self.check_now();
        }
    }

    pub fn check_now(&mut self) {
        if !Self::enabled() || self.busy() {
            return;
        }
        self.last_check = Some(Instant::now());
        self.state = UpdateState::Checking;
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(check()));
        });
    }

    pub fn start_update(&mut self) {
        let UpdateState::Available(rel) = self.state.clone() else {
            return;
        };
        if !Self::enabled() {
            return;
        }
        self.state = UpdateState::Downloading {
            done: 0,
            total: rel.asset_size,
        };
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = download(&rel, &tx);
            let _ = tx.send(Msg::Ready(r));
        });
    }

    /// Process worker messages. Returns true when a verified update is ready to apply.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut ready = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Checked(Ok(Some(r))) => self.state = UpdateState::Available(r),
                Msg::Checked(Ok(None)) => self.state = UpdateState::UpToDate,
                Msg::Checked(Err(e)) => self.state = UpdateState::Error(e),
                Msg::Progress(d, t) => {
                    if let UpdateState::Downloading { done, total } = &mut self.state {
                        *done = d;
                        *total = t;
                    }
                }
                Msg::Verifying => {
                    self.state = UpdateState::Verifying;
                }
                Msg::Ready(Ok(p)) => {
                    self.ready = Some(p);
                    self.state = UpdateState::Applying;
                    ready = true;
                }
                Msg::Ready(Err(e)) => self.state = UpdateState::Error(e),
            }
        }
        ready
    }

    /// Apply the verified download. On success the app must exit right away.
    pub fn apply(&mut self) -> Result<(), String> {
        let Some(p) = self.ready.take() else {
            return Err("Nothing to apply.".into());
        };
        let r = crate::install::apply_update(&p);
        if let Err(e) = &r {
            self.state = UpdateState::Error(e.clone());
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_parsing() {
        let h = "a".repeat(64);
        let s = format!(
            "{h}  FastFileRecovery-setup.exe\n{} *fast-file-recovery-linux-x64\n",
            "B".repeat(64)
        );
        assert_eq!(find_sum(&s, "FastFileRecovery-setup.exe"), Some(h));
        assert_eq!(
            find_sum(&s, "fast-file-recovery-linux-x64"),
            Some("b".repeat(64))
        );
        assert_eq!(find_sum(&s, "nope"), None);
    }

    #[test]
    fn only_github_https_urls() {
        assert!(trusted_url(
            "https://github.com/Akulav/fast-file-recovery/releases/download/v1/x"
        ));
        assert!(!trusted_url("http://github.com/x"));
        assert!(!trusted_url("https://github.com.evil.example/x"));
        assert!(!trusted_url("https://example.com/x"));
    }
}
