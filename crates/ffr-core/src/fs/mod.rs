//! Partition tables and quick-scan filesystem parsers.
//!
//! [`QuickScanner`] is the extension point for more filesystems (ext4, APFS, HFS+ …).

pub mod exfat;
pub mod fat;
pub mod ntfs;
pub mod partition;

use crate::carve::Confidence;
use crate::device::{FsType, PartitionInfo};
use crate::error::Result;
use crate::io::BlockRead;

/// A deleted file found in filesystem metadata.
#[derive(Clone, Debug, Default)]
pub struct QuickFile {
    pub name: String,
    /// Folder path inside the partition, using `/` separators (no leading slash).
    pub path: String,
    pub size: u64,
    /// Absolute device extents `(offset, len)`. `offset == u64::MAX` is a sparse run (zeros).
    pub extents: Vec<(u64, u64)>,
    /// Small files stored inside metadata (NTFS resident data).
    pub inline: Option<Vec<u8>>,
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub confidence: Confidence,
}

/// Lets a scanner check for cancel / pause and report progress (0.0–1.0).
pub trait ScanControl {
    /// Waits while paused; returns true when the scan should stop.
    fn should_stop(&self) -> bool;
    fn progress(&self, fraction: f32);
}

pub trait QuickScanner {
    fn scan(
        &self,
        r: &dyn BlockRead,
        part: &PartitionInfo,
        ctl: &dyn ScanControl,
        out: &mut dyn FnMut(QuickFile),
    ) -> Result<()>;
}

pub fn scanner_for(fs: &FsType) -> Option<Box<dyn QuickScanner>> {
    match fs {
        FsType::Ntfs => Some(Box::new(ntfs::Ntfs)),
        FsType::Fat12 | FsType::Fat16 | FsType::Fat32 => Some(Box::new(fat::Fat)),
        FsType::ExFat => Some(Box::new(exfat::ExFat)),
        _ => None,
    }
}

/// Read `len` bytes at `off` (short reads at the end are an error).
pub(crate) fn read_vec(r: &dyn BlockRead, off: u64, len: usize) -> Option<Vec<u8>> {
    let mut v = vec![0u8; len];
    let n = crate::io::read_full(r, off, &mut v).ok()?;
    (n == len).then_some(v)
}

/// Sanitize a name from disk for use in paths.
pub(crate) fn clean_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let t = t.trim().trim_end_matches('.').to_string();
    if t.is_empty() {
        "_".into()
    } else {
        t
    }
}
