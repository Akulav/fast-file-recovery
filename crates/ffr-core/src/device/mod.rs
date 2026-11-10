//! Device enumeration, read-only opening and locking.
//!
//! Platform code lives in `windows.rs`, `linux.rs` and `macos.rs`, each implementing the
//! [`Platform`] trait. This file holds the shared types and logic.

use crate::error::{CoreError, Result};
use crate::io::{AlignedBuf, BlockRead};
use crate::util::{align_down, align_up};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Os;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::Os;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Os;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod other;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use other::Os;

/// Stable per-session device id: `PhysicalDriveN`, `/dev/sdX`, `/dev/diskN` or `image:<path>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    PhysicalDisk,
    Volume,
    ImageFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusType {
    Nvme,
    Sata,
    Usb,
    Sd,
    Virtual,
    Unknown,
}

impl BusType {
    pub fn label(self) -> &'static str {
        match self {
            BusType::Nvme => "NVMe",
            BusType::Sata => "SATA",
            BusType::Usb => "USB",
            BusType::Sd => "SD card",
            BusType::Virtual => "Virtual",
            BusType::Unknown => "Disk",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsType {
    Ntfs,
    Fat12,
    Fat16,
    Fat32,
    ExFat,
    Ext,
    Btrfs,
    Xfs,
    Apfs,
    HfsPlus,
    Iso9660,
    Unknown,
    Other(String),
}

impl FsType {
    pub fn label(&self) -> &str {
        match self {
            FsType::Ntfs => "NTFS",
            FsType::Fat12 => "FAT12",
            FsType::Fat16 => "FAT16",
            FsType::Fat32 => "FAT32",
            FsType::ExFat => "exFAT",
            FsType::Ext => "ext4",
            FsType::Btrfs => "btrfs",
            FsType::Xfs => "XFS",
            FsType::Apfs => "APFS",
            FsType::HfsPlus => "HFS+",
            FsType::Iso9660 => "ISO 9660",
            FsType::Unknown => "RAW",
            FsType::Other(s) => s,
        }
    }

    pub fn from_name(s: &str) -> FsType {
        match s.to_ascii_lowercase().as_str() {
            "ntfs" => FsType::Ntfs,
            "fat12" => FsType::Fat12,
            "fat16" | "fat" => FsType::Fat16,
            "fat32" | "vfat" | "msdos" => FsType::Fat32,
            "exfat" => FsType::ExFat,
            "ext2" | "ext3" | "ext4" => FsType::Ext,
            "btrfs" => FsType::Btrfs,
            "xfs" => FsType::Xfs,
            "apfs" => FsType::Apfs,
            "hfs" | "hfs+" | "hfsplus" => FsType::HfsPlus,
            "iso9660" | "cdfs" | "udf" => FsType::Iso9660,
            "" | "raw" => FsType::Unknown,
            other => FsType::Other(other.to_string()),
        }
    }

    /// Whether the quick scan can read deleted entries from this filesystem.
    pub fn quick_scan_supported(&self) -> bool {
        matches!(
            self,
            FsType::Ntfs | FsType::Fat12 | FsType::Fat16 | FsType::Fat32 | FsType::ExFat
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionInfo {
    pub index: u32,
    pub offset: u64,
    pub len: u64,
    pub fs: FsType,
    pub label: Option<String>,
    pub mount_points: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    /// Raw path to open.
    pub path: PathBuf,
    pub kind: DeviceKind,
    pub model: String,
    pub serial: Option<String>,
    pub bus: BusType,
    pub size: u64,
    /// Logical sector size.
    pub sector_size: u32,
    /// Physical sector size, used for alignment.
    pub physical_sector_size: u32,
    /// Drives the I/O strategy (one sequential reader for spinning disks).
    pub rotational: Option<bool>,
    pub removable: bool,
    /// Holds the running OS: warn, cannot lock.
    pub is_system: bool,
    pub partitions: Vec<PartitionInfo>,
    /// Whether this process could open the device for reading.
    pub readable: bool,
}

impl DeviceInfo {
    /// A short title such as "Samsung SSD 980 · 1.00 TB".
    pub fn title(&self) -> String {
        format!("{} · {}", self.model, crate::util::human_size(self.size))
    }

    /// Spinning disk or USB: use one sequential reader.
    pub fn prefers_sequential(&self) -> bool {
        self.rotational == Some(true) || self.bus == BusType::Usb
    }
}

/// How to open a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockMode {
    /// Read-only, no locking.
    None,
    /// Lock the disk so the OS cannot write to it. `unmount` also unmounts its partitions
    /// (Linux, macOS) — the GUI asks the user before setting it.
    Lock { unmount: bool },
}

/// The one interface every platform module implements.
pub(crate) trait Platform {
    fn list_devices() -> Vec<DeviceInfo>;
    /// Open a raw device read-only (unbuffered where the OS supports it).
    fn open_raw(path: &Path) -> io::Result<File>;
    /// Device size from the OS (for raw devices `metadata().len()` is 0).
    fn device_size(f: &File) -> io::Result<u64>;
    /// Positional read.
    fn pread(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize>;
    /// Lock the disk's volumes. The returned guard releases them when dropped.
    fn lock(dev: &DeviceInfo, unmount: bool) -> Result<Box<dyn DiskLock>>;
    /// Physical disks that hold `path`.
    fn disks_of_path(path: &Path) -> Vec<DeviceId>;
    /// Whether `path` is on a network share.
    fn is_network_path(path: &Path) -> bool;
    /// Free bytes on the filesystem holding `path`.
    fn free_space(path: &Path) -> Option<u64>;
    /// Installed RAM in bytes.
    fn total_ram() -> Option<u64>;
    /// Change owner (no-op where it does not apply).
    fn chown(path: &Path, uid: u32, gid: u32);
}

/// Free bytes on the filesystem holding `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    Os::free_space(path)
}

/// Installed RAM in bytes.
pub fn total_ram() -> Option<u64> {
    Os::total_ram()
}

/// Give a folder tree to a user (used when running elevated on Linux/macOS).
///
/// Runs as root, so it never follows symbolic links: a link planted in the tree (or the tree
/// root swapped for a link) must not hand some other file to the user.
pub fn chown_tree(root: &Path, uid: u32, gid: u32) {
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    Os::chown(root, uid, gid);
    if !meta.is_dir() {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            chown_tree(&e.path(), uid, gid);
        }
    }
}

/// A held disk lock. Dropping it releases the lock (also on panic and cancel).
pub trait DiskLock: Send + Sync {
    /// Human description, e.g. "C:, D:" or "/dev/sdb (read-only)".
    fn describe(&self) -> String;
}

/// A read-only handle to a source. There is no write method in this type, and the compiler
/// enforces it (this example must fail to compile):
///
/// ```compile_fail
/// let dev = ffr_core::device::image_device(std::path::Path::new("disk.img")).unwrap();
/// let h = ffr_core::device::open_reader(&dev).unwrap();
/// h.write_at(0, &[0u8; 512]);
/// ```
///
/// The positive control (reading) compiles:
///
/// ```no_run
/// let dev = ffr_core::device::image_device(std::path::Path::new("disk.img")).unwrap();
/// let h = ffr_core::device::open_reader(&dev).unwrap();
/// let mut buf = [0u8; 512];
/// h.read_at(0, &mut buf).unwrap();
/// ```
pub struct SourceHandle {
    file: File,
    size: u64,
    sector: u32,
    align: usize,
    direct: bool,
    lock: Option<Box<dyn DiskLock>>,
}

impl SourceHandle {
    /// Read at `offset`. Handles the alignment rules of unbuffered I/O transparently.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let max = ((self.size - offset).min(buf.len() as u64)) as usize;
        let buf = &mut buf[..max];
        if !self.direct {
            return Os::pread(&self.file, buf, offset);
        }
        let a = self.align as u64;
        let aligned = offset.is_multiple_of(a)
            && (buf.len() as u64).is_multiple_of(a)
            && (buf.as_ptr() as usize).is_multiple_of(self.align);
        if aligned {
            return Os::pread(&self.file, buf, offset);
        }
        // Bounce through an aligned buffer.
        let start = align_down(offset, a);
        let end = align_up(offset + buf.len() as u64, a).min(align_up(self.size, a));
        let mut bounce = AlignedBuf::new((end - start) as usize, self.align.max(4096));
        let mut got = 0usize;
        while got < bounce.len() {
            match Os::pread(&self.file, &mut bounce[got..], start + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let skip = (offset - start) as usize;
        if got <= skip {
            return Ok(0);
        }
        let n = (got - skip).min(buf.len());
        buf[..n].copy_from_slice(&bounce[skip..skip + n]);
        Ok(n)
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn sector_size(&self) -> u32 {
        self.sector
    }

    /// Alignment used for direct reads (physical sector size, at least 512).
    pub fn alignment(&self) -> usize {
        self.align
    }

    pub fn is_locked(&self) -> bool {
        self.lock.is_some()
    }

    pub fn lock_description(&self) -> Option<String> {
        self.lock.as_ref().map(|l| l.describe())
    }
}

impl BlockRead for SourceHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        SourceHandle::read_at(self, offset, buf)
    }
    fn size(&self) -> u64 {
        self.size
    }
    fn sector_size(&self) -> u32 {
        self.sector
    }
}

/// List physical disks. Partition tables are read directly when the disk is readable.
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut devs = Os::list_devices();
    for d in &mut devs {
        if !d.readable {
            continue;
        }
        if let Ok(h) = open_source(d, LockMode::None) {
            let parsed = crate::fs::partition::detect(&h);
            if !parsed.is_empty() {
                d.partitions = merge_partitions(parsed, std::mem::take(&mut d.partitions));
            }
        }
    }
    devs
}

/// Keep the parsed table but copy labels and mount points the OS reported for the same offset.
fn merge_partitions(parsed: Vec<PartitionInfo>, os: Vec<PartitionInfo>) -> Vec<PartitionInfo> {
    parsed
        .into_iter()
        .map(|mut p| {
            if let Some(o) = os.iter().find(|o| o.offset == p.offset) {
                if p.label.is_none() {
                    p.label = o.label.clone();
                }
                p.mount_points = o.mount_points.clone();
                if p.fs == FsType::Unknown {
                    p.fs = o.fs.clone();
                }
            }
            p
        })
        .collect()
}

/// Treat a disk image file (`.img`, `.dd`, `.raw`, `.bin`, `.iso`) like a device.
pub fn image_device(path: &Path) -> Result<DeviceInfo> {
    let path = std::fs::canonicalize(path).map_err(CoreError::from_io)?;
    let meta = std::fs::metadata(&path).map_err(CoreError::from_io)?;
    if !meta.is_file() {
        return Err(CoreError::Invalid("That is not a file.".into()));
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Disk image".into());
    let mut info = DeviceInfo {
        id: DeviceId(format!("image:{}", path.display())),
        path: path.clone(),
        kind: DeviceKind::ImageFile,
        model: name,
        serial: None,
        bus: BusType::Virtual,
        size: meta.len(),
        sector_size: 512,
        physical_sector_size: 512,
        rotational: None,
        removable: false,
        is_system: false,
        partitions: Vec::new(),
        readable: true,
    };
    if let Ok(h) = open_source(&info, LockMode::None) {
        info.partitions = crate::fs::partition::detect(&h);
    }
    Ok(info)
}

/// Open a source read-only, optionally locking it first.
pub fn open_source(dev: &DeviceInfo, mode: LockMode) -> Result<SourceHandle> {
    if dev.kind == DeviceKind::ImageFile {
        let file = File::open(&dev.path).map_err(CoreError::from_io)?;
        let size = file.metadata().map_err(CoreError::from_io)?.len();
        return Ok(SourceHandle {
            file,
            size,
            sector: 512,
            align: 512,
            direct: false,
            lock: None,
        });
    }
    let lock = match mode {
        LockMode::Lock { unmount } if !dev.is_system => Some(Os::lock(dev, unmount)?),
        _ => None,
    };
    let file = Os::open_raw(&dev.path).map_err(CoreError::from_io)?;
    let size = Os::device_size(&file).unwrap_or(dev.size);
    let size = if size == 0 { dev.size } else { size };
    let align = dev.physical_sector_size.max(dev.sector_size).max(512) as usize;
    Ok(SourceHandle {
        file,
        size,
        sector: dev.sector_size.max(512),
        align,
        direct: true,
        lock,
    })
}

/// An extra read-only handle without locking (for resolvers, previews and recovery).
pub fn open_reader(dev: &DeviceInfo) -> Result<SourceHandle> {
    open_source(dev, LockMode::None)
}

/// Physical disks that hold `path` (empty when unknown).
pub fn disks_of_path(path: &Path) -> Vec<DeviceId> {
    Os::disks_of_path(path)
}

pub fn is_network_path(path: &Path) -> bool {
    Os::is_network_path(path)
}

/// Fingerprint used to match a saved session to a device: model + serial + size + first-MiB hash.
pub fn fingerprint(dev: &DeviceInfo, src: &dyn BlockRead) -> String {
    let mut first = vec![0u8; (1 << 20).min(src.size() as usize)];
    let n = crate::io::read_full(src, 0, &mut first).unwrap_or(0);
    let hash = blake3::hash(&first[..n]);
    format!(
        "{}|{}|{}|{}",
        dev.model,
        dev.serial.as_deref().unwrap_or(""),
        src.size(),
        &hash.to_hex()[..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_files_read_and_report_size() {
        let dir = std::env::temp_dir().join(format!("ffr-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.img");
        std::fs::write(&p, vec![7u8; 4096]).unwrap();
        let d = image_device(&p).unwrap();
        assert_eq!(d.size, 4096);
        let h = open_source(&d, LockMode::Lock { unmount: true }).unwrap();
        assert!(!h.is_locked());
        let mut b = [0u8; 100];
        assert_eq!(h.read_at(4000, &mut b).unwrap(), 96);
        assert_eq!(b[0], 7);
        std::fs::remove_dir_all(&dir).ok();
    }
}
