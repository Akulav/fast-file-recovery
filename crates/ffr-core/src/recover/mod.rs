//! Recovery: destination checks, naming, layouts, the writer pool and the manifest.

mod writer;

pub use writer::start_recovery;

use crate::carve::FileMeta;
use crate::device::{self, DeviceId, DeviceInfo, DeviceKind};
use crate::fs::clean_name;
use crate::io::{read_full, BlockRead};
use crate::scan::{Hit, HitExtra};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// `<dest>/FFR_<date>_<time>/<Category>/<ext>/`
    ByType,
    /// Quick-scan hits keep their folder tree; carved ones go to `_Carved/<ext>/`.
    OriginalPaths,
    Flat,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::ByType, Layout::OriginalPaths, Layout::Flat];
    pub fn label(self) -> &'static str {
        match self {
            Layout::ByType => "By type",
            Layout::OriginalPaths => "Original paths",
            Layout::Flat => "Flat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartialPolicy {
    /// Files with unreadable sectors get a `.partial` suffix.
    Suffix,
    /// Written with zero-fill and flagged in the manifest only.
    ZeroFill,
}

#[derive(Clone, Debug)]
pub struct RecoverOptions {
    pub dest: PathBuf,
    pub layout: Layout,
    pub partial_policy: PartialPolicy,
    pub write_timestamps: bool,
    pub retries: u32,
    /// Give recovered files to this user (Linux/macOS when elevated).
    pub chown: Option<(u32, u32)>,
    /// Destination is a spinning disk (2 writers instead of 4).
    pub dest_rotational: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverSummary {
    pub recovered: u64,
    pub partial: u64,
    pub failed: u64,
    pub bytes: u64,
    pub folder: PathBuf,
    pub report_csv: PathBuf,
    pub report_json: PathBuf,
    pub elapsed_secs: f64,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum RecoverEvent {
    Progress {
        files_done: u64,
        files_total: u64,
        bytes_done: u64,
        bytes_total: u64,
        bytes_per_sec: f64,
        current: String,
    },
    Done(RecoverSummary),
    Failed(String),
}

// ---------------------------------------------------------------- destination checks

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem {
    pub ok: bool,
    /// A failed blocking check disables the Recover button.
    pub blocking: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct DestCheck {
    pub items: Vec<CheckItem>,
}

impl DestCheck {
    pub fn can_start(&self) -> bool {
        self.items.iter().all(|i| i.ok || !i.blocking)
    }
    pub fn first_blocker(&self) -> Option<&CheckItem> {
        self.items.iter().find(|i| !i.ok && i.blocking)
    }
}

/// Pure rule: is the destination on the source disk?
pub fn same_disk(source: &DeviceInfo, dest_disks: &[DeviceId]) -> bool {
    dest_disks.contains(&source.id)
}

/// True when two paths name the same existing file or folder (after resolving links).
pub fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => {
            if cfg!(windows) {
                x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
            } else {
                x == y
            }
        }
        _ => false,
    }
}

/// Hard rule used right before writing: `Err` when recovered files must not go to `dest`.
pub fn ensure_safe_destination(source: &DeviceInfo, dest: &Path) -> Result<(), String> {
    let canon = std::fs::canonicalize(dest)
        .map_err(|_| "The destination folder doesn't exist.".to_string())?;
    if source.kind == DeviceKind::ImageFile {
        if same_path(&canon, &source.path) {
            return Err("The destination is the image file itself.".into());
        }
        return Ok(());
    }
    let disks = device::disks_of_path(&canon);
    if disks.is_empty() {
        return Err(
            "Couldn't tell which disk this folder is on. Pick a folder on another disk.".into(),
        );
    }
    if same_disk(source, &disks) {
        return Err(
            "The destination is on the disk you are recovering from. Pick a different disk.".into(),
        );
    }
    Ok(())
}

/// Make sure `src` is still the disk the scan was made from. Disk numbers can change after a
/// reboot or re-plug, and reading (or worse, writing next to) the wrong disk must never happen.
pub fn verify_source(
    dev: &DeviceInfo,
    fingerprint: &str,
    src: &dyn BlockRead,
) -> Result<(), String> {
    const CHANGED: &str = "The disk this scan was made from is not connected, or it changed. \
         Connect it and scan again.";
    if !fingerprint.is_empty() {
        return if device::fingerprint(dev, src) == fingerprint {
            Ok(())
        } else {
            Err(CHANGED.into())
        };
    }
    // Older saved scans have no fingerprint: match the disk's identity instead.
    if dev.kind == DeviceKind::ImageFile {
        return Ok(());
    }
    let now = device::list_devices();
    match now.iter().find(|d| d.id == dev.id) {
        Some(d) if d.model == dev.model && d.serial == dev.serial && d.size == dev.size => Ok(()),
        _ => Err(CHANGED.into()),
    }
}

/// Check a destination folder. `needed` is the total size of the selection.
pub fn check_destination(source: &DeviceInfo, dest: &Path, needed: u64) -> DestCheck {
    let mut c = DestCheck::default();
    let mut push =
        |ok: bool, blocking: bool, text: String| c.items.push(CheckItem { ok, blocking, text });
    let canon = match std::fs::canonicalize(dest) {
        Ok(p) => p,
        Err(_) => {
            push(false, true, "The destination folder doesn't exist.".into());
            return c;
        }
    };
    if source.kind == DeviceKind::ImageFile {
        let ok = !same_path(&canon, &source.path);
        push(
            ok,
            true,
            if ok {
                "The destination is not the image file".into()
            } else {
                "The destination is the image file itself".into()
            },
        );
    } else {
        let disks = device::disks_of_path(&canon);
        if disks.is_empty() {
            push(
                false,
                true,
                "Couldn't tell which disk this folder is on. Pick a folder on another disk.".into(),
            );
        } else if same_disk(source, &disks) {
            push(
                false,
                true,
                "The destination is on the disk you are recovering from. Pick a different disk."
                    .into(),
            );
        } else {
            push(true, true, "Destination is on a different disk".into());
        }
    }
    let writable = {
        let probe = canon.join(format!(".ffr-write-test-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    };
    push(
        writable,
        true,
        if writable {
            "The folder is writable".into()
        } else {
            "Can't write to this folder".into()
        },
    );
    let want = needed.saturating_add(needed / 100);
    match device::free_space(&canon) {
        Some(free) if free >= want => push(
            true,
            true,
            format!("Enough free space ({} free)", crate::util::human_size(free)),
        ),
        Some(free) => push(
            false,
            true,
            format!(
                "Not enough free space: {} needed, {} free",
                crate::util::human_size(want),
                crate::util::human_size(free)
            ),
        ),
        None => push(true, false, "Free space unknown".into()),
    }
    if device::is_network_path(&canon) {
        push(
            false,
            false,
            "This is a network folder; recovery may be slow".into(),
        );
    }
    c
}

// ---------------------------------------------------------------- naming

/// Name for a carved hit: `<ext>_<offset-hex>[_<WxH>|_<title>].<ext>`.
pub fn carved_name(hit: &Hit, meta: Option<&FileMeta>) -> String {
    let ext = hit.kind().ext;
    let mut s = format!("{ext}_{:x}", hit.offset);
    if let Some(m) = meta {
        if m.width > 0 && m.height > 0 {
            s.push_str(&format!("_{}x{}", m.width, m.height));
        } else if let Some(t) = &m.title {
            let t: String = clean_name(t).chars().take(40).collect();
            s.push('_');
            s.push_str(&t);
        }
    }
    format!("{s}.{ext}")
}

/// Pieces of a hit's data: `(offset, len)`, `u64::MAX` offset meaning zeros.
pub fn segments(hit: &Hit, extra: Option<&HitExtra>) -> Vec<(u64, u64)> {
    match extra {
        Some(e) if !e.extents.is_empty() => e.extents.clone(),
        _ => vec![(hit.offset, hit.len)],
    }
}

/// First `max` bytes of a hit (for previews).
pub fn read_prefix(
    src: &dyn BlockRead,
    hit: &Hit,
    extra: Option<&HitExtra>,
    max: usize,
) -> std::io::Result<Vec<u8>> {
    if let Some(b) = extra.and_then(|e| e.inline.as_ref()) {
        return Ok(b[..b.len().min(max)].to_vec());
    }
    let want = (hit.len.min(max as u64)) as usize;
    let mut out = Vec::with_capacity(want);
    for (off, len) in segments(hit, extra) {
        if out.len() >= want {
            break;
        }
        let n = (len as usize).min(want - out.len());
        if off == u64::MAX {
            out.resize(out.len() + n, 0);
            continue;
        }
        let at = out.len();
        out.resize(at + n, 0);
        let got = read_full(src, off, &mut out[at..])?;
        out.truncate(at + got);
        if got < n {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{BusType, DeviceKind};

    fn dev(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(id.into()),
            path: PathBuf::from(id),
            kind: DeviceKind::PhysicalDisk,
            model: "Test".into(),
            serial: None,
            bus: BusType::Sata,
            size: 1 << 30,
            sector_size: 512,
            physical_sector_size: 512,
            rotational: Some(false),
            removable: false,
            is_system: false,
            partitions: Vec::new(),
            readable: true,
        }
    }

    #[test]
    fn destination_on_source_disk_is_refused() {
        let src = dev("PhysicalDrive1");
        assert!(same_disk(&src, &[DeviceId("PhysicalDrive1".into())]));
        assert!(same_disk(
            &src,
            &[
                DeviceId("PhysicalDrive0".into()),
                DeviceId("PhysicalDrive1".into())
            ]
        ));
        assert!(!same_disk(&src, &[DeviceId("PhysicalDrive0".into())]));
    }

    #[test]
    fn carved_names() {
        let hit = Hit {
            offset: 0x1F400,
            len: 10,
            extra: u32::MAX,
            kind: crate::kinds::lookup("jpg"),
            sig: 0,
            confidence: crate::carve::Confidence::High,
            flags: 0,
        };
        assert_eq!(carved_name(&hit, None), "jpg_1f400.jpg");
        let m = FileMeta::dims(640, 480);
        assert_eq!(carved_name(&hit, m.as_ref()), "jpg_1f400_640x480.jpg");
    }
}
