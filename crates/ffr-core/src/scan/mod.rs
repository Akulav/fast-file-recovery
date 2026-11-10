//! Scanning: types, events, control handle and session files. The pipeline is in `engine.rs`.

mod engine;
mod session;

pub use engine::{build_matcher, match_chunk, Matcher};
pub use session::{load_session, save_session};

use crate::carve::{Confidence, FileMeta};
use crate::device::DeviceInfo;
use crate::error::ScanError;
use crate::kinds;
use crossbeam_channel::Receiver;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub type HitId = u32;

/// Hit flags.
pub mod flags {
    /// Found by the quick scan (filesystem metadata).
    pub const QUICK: u8 = 1;
    /// The file runs past the end of the device or the structure is cut short.
    pub const TRUNCATED: u8 = 2;
    /// Overlaps unreadable sectors.
    pub const BAD_SECTORS: u8 = 4;
    /// Stored in more than one extent.
    pub const FRAGMENTED: u8 = 8;
    /// Data is stored inside filesystem metadata.
    pub const RESIDENT: u8 = 16;
}

/// One found file. Compact (32 bytes) so millions fit in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub offset: u64,
    pub len: u64,
    /// Index into the session's `extras`, or `u32::MAX`.
    pub extra: u32,
    /// Index into [`kinds::KINDS`].
    pub kind: u16,
    /// Signature index, or `u16::MAX` for quick-scan hits.
    pub sig: u16,
    pub confidence: Confidence,
    pub flags: u8,
}

impl Hit {
    pub fn kind(&self) -> &'static kinds::Kind {
        kinds::get(self.kind)
    }
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.len)
    }
}

/// Details for hits that have them (names, paths, extents, metadata).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HitExtra {
    pub name: Option<String>,
    pub path: Option<String>,
    /// Absolute extents; `u64::MAX` offset = sparse (zeros). Empty = contiguous `[offset, len)`.
    pub extents: Vec<(u64, u64)>,
    pub inline: Option<Vec<u8>>,
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub meta: Option<FileMeta>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanMode {
    Quick,
    Deep,
    Full,
}

impl ScanMode {
    pub fn label(self) -> &'static str {
        match self {
            ScanMode::Quick => "Quick",
            ScanMode::Deep => "Deep",
            ScanMode::Full => "Full",
        }
    }
}

/// What to scan: a device (or image file), optionally limited to a byte range (a partition).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpec {
    pub device: DeviceInfo,
    pub range: Option<Range<u64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanOptions {
    pub mode: ScanMode,
    /// Enabled signature ids.
    pub enabled: BTreeSet<String>,
    pub chunk_size: usize,
    /// Reader threads for SSD/NVMe; 0 = automatic (4, or 1 for spinning/USB disks).
    pub queue_depth: usize,
    pub retries: u32,
    pub cluster_aligned_only: bool,
    /// Lock the disk for the length of the scan.
    pub lock: bool,
    /// Also unmount partitions (Linux/macOS; asked of the user first).
    pub unmount: bool,
    /// Where checkpoints are written (`<config>/sessions`).
    pub checkpoint_dir: Option<PathBuf>,
    pub session_id: String,
    /// Continue a previous session (its scanned ranges are skipped).
    #[serde(skip)]
    pub resume: Option<Box<Session>>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            mode: ScanMode::Full,
            enabled: crate::signatures::default_ids(),
            chunk_size: 16 << 20,
            queue_depth: 0,
            retries: 2,
            cluster_aligned_only: true,
            lock: true,
            unmount: false,
            checkpoint_dir: None,
            session_id: new_session_id(),
            resume: None,
        }
    }
}

/// A unique, sortable id like `20250101-120000-1234`.
pub fn new_session_id() -> String {
    let (y, mo, d, h, mi, s) = crate::util::now_utc();
    format!(
        "{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}-{:04}",
        std::process::id() % 10_000
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Preparing,
    Locking,
    QuickScan,
    DeepScan,
    Finishing,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Preparing => "Preparing",
            Phase::Locking => "Locking the disk",
            Phase::QuickScan => "Reading file system records",
            Phase::DeepScan => "Searching for file signatures",
            Phase::Finishing => "Finishing",
        }
    }
}

/// Batch of new hits. `extras[i]` belongs to the hit whose `extra` is `first_extra + i`.
#[derive(Clone, Debug, Default)]
pub struct HitsBatch {
    pub hits: Vec<Hit>,
    pub extras: Vec<HitExtra>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub hits: u64,
    pub bytes_scanned: u64,
    pub elapsed: Duration,
    pub bad_sectors: u64,
    pub cancelled: bool,
    pub locked: bool,
}

#[derive(Clone, Debug)]
pub enum ScanEvent {
    Phase(Phase),
    Locked(String),
    /// Identity of the source (model + serial + size + first-MiB hash), sent once at the start
    /// so the GUI's copy of the session can be checked before recovery.
    Fingerprint(String),
    Progress {
        bytes_done: u64,
        bytes_total: u64,
        bytes_per_sec: f64,
        eta: Option<Duration>,
    },
    Hits(HitsBatch),
    BadRange(Range<u64>),
    /// One byte per cell: 0 unread, 1 read, 2 contains hits, 3 bad.
    SectorMap(Vec<u8>),
    Warning(String),
    Done(Summary),
    Failed(ScanError),
}

pub const SECTOR_MAP_CELLS: usize = 4000;

/// Cancel / pause control shared between the GUI and the engine.
#[derive(Clone, Default)]
pub struct ScanHandle {
    inner: Arc<HandleInner>,
}

#[derive(Default)]
struct HandleInner {
    cancel: AtomicBool,
    pause: (Mutex<bool>, Condvar),
}

impl ScanHandle {
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
        self.resume();
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancel.load(Ordering::SeqCst)
    }
    pub fn pause(&self) {
        if let Ok(mut p) = self.inner.pause.0.lock() {
            *p = true;
        }
    }
    pub fn resume(&self) {
        if let Ok(mut p) = self.inner.pause.0.lock() {
            *p = false;
        }
        self.inner.pause.1.notify_all();
    }
    pub fn is_paused(&self) -> bool {
        self.inner.pause.0.lock().map(|p| *p).unwrap_or(false)
    }
    /// Block while paused. Returns true if the scan should stop.
    pub fn wait_if_paused(&self) -> bool {
        let (m, cv) = &self.inner.pause;
        if let Ok(mut p) = m.lock() {
            while *p && !self.is_cancelled() {
                match cv.wait_timeout(p, Duration::from_millis(200)) {
                    Ok((g, _)) => p = g,
                    Err(_) => return self.is_cancelled(),
                }
            }
        }
        self.is_cancelled()
    }
}

/// Everything about one scan: kept by the GUI, saved as `.ffrscan` checkpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub source: SourceSpec,
    pub mode: ScanMode,
    pub fingerprint: String,
    pub enabled: BTreeSet<String>,
    pub cluster_aligned_only: bool,
    pub hits: Vec<Hit>,
    pub extras: Vec<HitExtra>,
    pub bad_ranges: Vec<Range<u64>>,
    /// Contiguous ranges the deep scan has finished.
    pub scanned: Vec<Range<u64>>,
    pub quick_done: bool,
    pub finished: bool,
    pub created_unix: i64,
}

impl Session {
    pub fn new(id: String, source: SourceSpec, mode: ScanMode) -> Self {
        Self {
            id,
            source,
            mode,
            fingerprint: String::new(),
            enabled: BTreeSet::new(),
            cluster_aligned_only: true,
            hits: Vec::new(),
            extras: Vec::new(),
            bad_ranges: Vec::new(),
            scanned: Vec::new(),
            quick_done: false,
            finished: false,
            created_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }

    pub fn extra(&self, hit: &Hit) -> Option<&HitExtra> {
        self.extras.get(hit.extra as usize)
    }

    /// Append a batch from a `ScanEvent::Hits`.
    pub fn apply(&mut self, batch: HitsBatch) {
        self.extras.extend(batch.extras);
        self.hits.extend(batch.hits);
    }

    /// Display name: original name, or the generated carve name.
    pub fn display_name(&self, hit: &Hit) -> String {
        if let Some(n) = self.extra(hit).and_then(|e| e.name.clone()) {
            return n;
        }
        crate::recover::carved_name(hit, self.extra(hit).and_then(|e| e.meta.as_ref()))
    }

    /// Display extension (from the original name when there is one).
    pub fn display_ext(&self, hit: &Hit) -> String {
        if let Some(n) = self.extra(hit).and_then(|e| e.name.as_ref()) {
            if let Some((_, e)) = n.rsplit_once('.') {
                if !e.is_empty() && e.len() <= 8 {
                    return e.to_ascii_lowercase();
                }
            }
        }
        hit.kind().ext.to_string()
    }

    pub fn bytes_scanned(&self) -> u64 {
        self.scanned.iter().map(|r| r.end - r.start).sum()
    }
}

/// Start a scan on a worker thread. Events arrive on the returned channel, batched to at
/// most 10 per second.
pub fn start_scan(source: SourceSpec, opts: ScanOptions) -> (ScanHandle, Receiver<ScanEvent>) {
    engine::start(source, opts)
}
