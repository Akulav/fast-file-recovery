//! The scan pipeline:
//!
//! ```text
//! reader thread(s) ──chunks──▶ match + resolve workers (rayon, N = cores) ──results──▶ aggregator
//!        ▲                                         │                                   │
//!        └────────────── buffers recycled ◀────────┘                    events (≤ 10 Hz)
//! ```
//!
//! Readers fill aligned buffers from a fixed pool (backpressure keeps memory bounded). Workers
//! run the signature matcher over each chunk plus a small overlap, resolve every candidate's
//! size, and return the buffer. The aggregator puts results back in disk order, removes
//! duplicates, tracks progress and writes checkpoints.

use super::{
    flags, Hit, HitExtra, HitsBatch, Phase, ScanEvent, ScanHandle, ScanMode, ScanOptions, Session,
    SourceSpec, Summary, SECTOR_MAP_CELLS,
};
use crate::carve::{self, Confidence, Reader};
use crate::device::{self, DeviceKind, LockMode, SourceHandle};
use crate::error::{CoreError, Result, ScanError};
use crate::fs::{self, QuickFile, ScanControl};
use crate::io::{read_with_retry, BadSectorPolicy, BlockRead, BufferPool, PooledBuf};
use crate::kinds;
use crate::signatures::{self, Signature};
use crate::util::align_up;
use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- matcher

/// Signature matcher for one set of enabled signatures.
///
/// Signatures that only start on 512-byte boundaries (when "Only cluster-aligned starts" is on)
/// are checked directly at each boundary, which is far cheaper than scanning every byte. All
/// other signatures go through one Aho-Corasick DFA in overlapping mode.
pub struct Matcher {
    ac: Option<AhoCorasick>,
    targets: Vec<Vec<(u16, u64)>>,
    /// Aligned patterns grouped by magic offset, keyed by their first two bytes.
    aligned: Vec<AlignedGroup>,
    /// Bytes past the owned region needed so no magic is cut at a chunk boundary.
    pub overlap: usize,
}

type PatList = Vec<(&'static [u8], u16)>;
/// (magic offset, patterns keyed by their first two bytes, 1-byte patterns)
type AlignedGroup = (u64, HashMap<[u8; 2], PatList>, PatList);

pub fn build_matcher(enabled: &BTreeSet<String>, aligned_only: bool) -> Option<Matcher> {
    let all = signatures::all();
    let mut pats: Vec<Vec<u8>> = Vec::new();
    let mut ids: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut targets: Vec<Vec<(u16, u64)>> = Vec::new();
    let mut aligned: BTreeMap<u64, Vec<(&'static [u8], u16)>> = BTreeMap::new();
    let mut max_len = 1usize;
    for (i, s) in all.iter().enumerate() {
        if !enabled.contains(s.id) {
            continue;
        }
        for &m in s.magic {
            max_len = max_len.max(m.len());
            if aligned_only && s.sector_aligned {
                aligned
                    .entry(s.magic_offset)
                    .or_default()
                    .push((m, i as u16));
                continue;
            }
            let id = *ids.entry(m.to_vec()).or_insert_with(|| {
                pats.push(m.to_vec());
                targets.push(Vec::new());
                pats.len() - 1
            });
            targets[id].push((i as u16, s.magic_offset));
        }
    }
    if pats.is_empty() && aligned.is_empty() {
        return None;
    }
    let ac = if pats.is_empty() {
        None
    } else {
        Some(
            AhoCorasick::builder()
                .match_kind(MatchKind::Standard)
                .kind(Some(AhoCorasickKind::DFA))
                .build(&pats)
                .ok()?,
        )
    };
    let aligned = aligned
        .into_iter()
        .map(|(off, list)| {
            let mut by2: HashMap<[u8; 2], Vec<(&'static [u8], u16)>> = HashMap::new();
            let mut short = Vec::new();
            for (m, s) in list {
                if m.len() >= 2 {
                    by2.entry([m[0], m[1]]).or_default().push((m, s));
                } else {
                    short.push((m, s));
                }
            }
            (off, by2, short)
        })
        .collect();
    Some(Matcher {
        ac,
        targets,
        aligned,
        overlap: max_len.saturating_sub(1),
    })
}

/// A raw match: a possible file start for one signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Candidate {
    pub start: u64,
    pub sig: u16,
}

/// Find candidates in `data` (which starts at `chunk_start`). Only magics that begin inside the
/// first `own_len` bytes are reported, so neighbouring chunks never report the same hit.
pub fn match_chunk(
    m: &Matcher,
    chunk_start: u64,
    data: &[u8],
    own_len: usize,
    out: &mut Vec<Candidate>,
) {
    let own_len = own_len.min(data.len());
    if let Some(ac) = &m.ac {
        for mat in ac.find_overlapping_iter(data) {
            let p = mat.start();
            if p >= own_len {
                continue;
            }
            let pos = chunk_start + p as u64;
            for &(sig, moff) in &m.targets[mat.pattern().as_usize()] {
                if pos >= moff {
                    out.push(Candidate {
                        start: pos - moff,
                        sig,
                    });
                }
            }
        }
    }
    let own_end = chunk_start + own_len as u64;
    for (moff, by2, short) in &m.aligned {
        // Starts s (multiple of 512) whose magic position s + moff lies in the owned region.
        let lo = chunk_start.saturating_sub(*moff);
        let mut s = align_up(lo, 512);
        while s + moff < own_end {
            if s + moff >= chunk_start {
                let i = (s + moff - chunk_start) as usize;
                if let Some(k) = data.get(i..i + 2) {
                    if let Some(list) = by2.get(&[k[0], k[1]]) {
                        for &(pat, sig) in list {
                            if data.get(i..i + pat.len()) == Some(pat) {
                                out.push(Candidate { start: s, sig });
                            }
                        }
                    }
                }
                for &(pat, sig) in short {
                    if data.get(i..i + pat.len()) == Some(pat) {
                        out.push(Candidate { start: s, sig });
                    }
                }
            }
            s += 512;
        }
    }
}

/// A resolved file from one chunk.
#[derive(Clone, Debug)]
pub struct Found {
    pub start: u64,
    pub len: u64,
    pub sig: u16,
    pub kind: u16,
    pub confidence: Confidence,
    pub truncated: bool,
    pub meta: Option<carve::FileMeta>,
}

/// Resolve candidates (sorted by start). Hits that lie inside a file already found for the
/// same signature are skipped (e.g. inner ZIP entries, nested JPEG thumbnails).
pub fn resolve_candidates(
    src: &dyn BlockRead,
    chunk_start: u64,
    data: &[u8],
    cands: &mut [Candidate],
) -> Vec<Found> {
    let all = signatures::all();
    cands.sort_unstable();
    let mut covered: HashMap<u16, u64> = HashMap::new();
    let mut r = Reader::new(src, chunk_start, data);
    let mut out = Vec::new();
    let mut last: Option<Candidate> = None;
    for &c in cands.iter() {
        if last == Some(c) {
            continue;
        }
        last = Some(c);
        if covered.get(&c.sig).is_some_and(|&end| c.start < end) {
            continue;
        }
        let sig: &Signature = &all[c.sig as usize];
        r.reset_limits(512 << 20, Duration::from_secs(4));
        if let Some(res) = carve::resolve(sig, &mut r, c.start) {
            if res.confidence >= Confidence::Medium {
                covered.insert(c.sig, c.start + res.len);
            }
            out.push(Found {
                start: c.start,
                len: res.len,
                sig: c.sig,
                kind: res.kind,
                confidence: res.confidence,
                truncated: res.truncated,
                meta: res.meta,
            });
        }
    }
    out
}

// ---------------------------------------------------------------- engine

pub(super) fn start(source: SourceSpec, opts: ScanOptions) -> (ScanHandle, Receiver<ScanEvent>) {
    let handle = ScanHandle::default();
    let (tx, rx) = unbounded();
    let h = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("ffr-scan".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| run(source, opts, &h, &tx)));
            let ev = match result {
                Ok(Ok(summary)) => ScanEvent::Done(summary),
                Ok(Err(e)) => ScanEvent::Failed(e.into()),
                Err(p) => {
                    let msg = p
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown".into());
                    tracing::error!("scan engine panicked: {msg}");
                    ScanEvent::Failed(ScanError::Other(format!(
                        "The scan stopped because of an internal error ({msg}). The disk was released."
                    )))
                }
            };
            let _ = tx.send(ev);
        });
    if let Err(e) = spawned {
        tracing::error!("could not start scan thread: {e}");
    }
    (handle, rx)
}

const SYSTEM_DISK_WARNING: &str = "The system is running from this disk, so it can't be locked. New writes may overwrite deleted data. Stop using the computer while scanning, or make an image from another computer.";

fn run(
    source: SourceSpec,
    mut opts: ScanOptions,
    h: &ScanHandle,
    tx: &Sender<ScanEvent>,
) -> Result<Summary> {
    let t0 = Instant::now();
    let send = |e| {
        let _ = tx.send(e);
    };
    send(ScanEvent::Phase(Phase::Preparing));
    let dev = &source.device;
    let want_lock = opts.lock && dev.kind == DeviceKind::PhysicalDisk;
    if want_lock && dev.is_system {
        send(ScanEvent::Warning(SYSTEM_DISK_WARNING.into()));
    }
    let mode = if want_lock && !dev.is_system {
        send(ScanEvent::Phase(Phase::Locking));
        LockMode::Lock {
            unmount: opts.unmount,
        }
    } else {
        LockMode::None
    };
    let src = Arc::new(device::open_source(dev, mode)?);
    if let Some(d) = src.lock_description() {
        send(ScanEvent::Locked(d));
    }
    let size = src.size();
    let range = match &source.range {
        Some(r) => r.start.min(size)..r.end.min(size),
        None => 0..size,
    };
    if range.is_empty() {
        return Err(CoreError::Invalid(
            "There is nothing to scan on this source.".into(),
        ));
    }
    let fingerprint = device::fingerprint(dev, &*src);
    let mut session = match opts.resume.take() {
        Some(s) if s.fingerprint == fingerprint && s.source.range == source.range => *s,
        Some(_) => {
            send(ScanEvent::Warning(
                "This disk changed since the scan was saved, so the scan starts over.".into(),
            ));
            Session::new(opts.session_id.clone(), source.clone(), opts.mode)
        }
        None => Session::new(opts.session_id.clone(), source.clone(), opts.mode),
    };
    session.fingerprint = fingerprint;
    session.enabled = opts.enabled.clone();
    session.cluster_aligned_only = opts.cluster_aligned_only;
    session.mode = opts.mode;
    session.finished = false;
    let checkpoint = opts
        .checkpoint_dir
        .as_ref()
        .map(|d| d.join(format!("{}.ffrscan", session.id)));
    let agg = RefCell::new(Agg::new(
        tx,
        session,
        range.clone(),
        checkpoint,
        src.is_locked(),
    ));
    agg.borrow_mut().emit_existing();

    let mode = opts.mode;
    if matches!(mode, ScanMode::Quick | ScanMode::Full) && !agg.borrow().s.quick_done {
        send(ScanEvent::Phase(Phase::QuickScan));
        quick_scan(&src, &range, h, &agg);
        if !h.is_cancelled() {
            agg.borrow_mut().s.quick_done = true;
        }
    }
    if matches!(mode, ScanMode::Deep | ScanMode::Full) && !h.is_cancelled() {
        send(ScanEvent::Phase(Phase::DeepScan));
        deep_scan(src.clone(), dev, &range, &opts, h, &agg)?;
    }
    send(ScanEvent::Phase(Phase::Finishing));
    let mut agg = agg.into_inner();
    let cancelled = h.is_cancelled();
    agg.s.finished = !cancelled;
    agg.emit(true);
    agg.checkpoint();
    let bad_sectors = agg
        .s
        .bad_ranges
        .iter()
        .map(|r| (r.end - r.start) / src.sector_size().max(1) as u64)
        .sum();
    Ok(Summary {
        hits: agg.s.hits.len() as u64,
        bytes_scanned: agg.s.bytes_scanned(),
        elapsed: t0.elapsed(),
        bad_sectors,
        cancelled,
        locked: agg.locked,
    })
}

// ---------------------------------------------------------------- aggregator

struct Agg<'a> {
    tx: &'a Sender<ScanEvent>,
    s: Session,
    pending: HitsBatch,
    pending_bad: Vec<Range<u64>>,
    map: Vec<u8>,
    map_dirty: bool,
    range: Range<u64>,
    bytes_done: u64,
    rate: VecDeque<(Instant, u64)>,
    last_emit: Instant,
    last_checkpoint: Instant,
    checkpoint_path: Option<PathBuf>,
    quick_starts: HashSet<u64>,
    covered: HashMap<u16, u64>,
    bad_run: (u64, u64),
    warned_bad: bool,
    was_paused: bool,
    locked: bool,
}

impl<'a> Agg<'a> {
    fn new(
        tx: &'a Sender<ScanEvent>,
        s: Session,
        range: Range<u64>,
        checkpoint_path: Option<PathBuf>,
        locked: bool,
    ) -> Self {
        Self {
            tx,
            s,
            pending: HitsBatch::default(),
            pending_bad: Vec::new(),
            map: vec![0; SECTOR_MAP_CELLS],
            map_dirty: true,
            range,
            bytes_done: 0,
            rate: VecDeque::new(),
            last_emit: Instant::now(),
            last_checkpoint: Instant::now(),
            checkpoint_path,
            quick_starts: HashSet::new(),
            covered: HashMap::new(),
            bad_run: (0, 0),
            warned_bad: false,
            was_paused: false,
            locked,
        }
    }

    fn total(&self) -> u64 {
        self.range.end - self.range.start
    }

    /// Re-send hits from a resumed session and mark its progress.
    fn emit_existing(&mut self) {
        let hits = self.s.hits.clone();
        for hit in &hits {
            if hit.has(flags::QUICK) {
                self.quick_starts.insert(hit.offset);
            }
            self.mark(hit.offset..hit.end(), 2);
        }
        let scanned: Vec<_> = self.s.scanned.clone();
        for r in scanned {
            self.mark(r.clone(), 1);
            self.bytes_done += r.end - r.start;
        }
        for r in self.s.bad_ranges.clone() {
            self.mark(r, 3);
        }
        if !hits.is_empty() {
            let _ = self.tx.send(ScanEvent::Hits(HitsBatch {
                hits,
                extras: self.s.extras.clone(),
            }));
        }
    }

    fn mark(&mut self, r: Range<u64>, state: u8) {
        let total = self.total().max(1);
        if r.end <= self.range.start || r.start >= self.range.end {
            return;
        }
        let a = r.start.max(self.range.start) - self.range.start;
        let b = r.end.min(self.range.end) - self.range.start;
        let n = SECTOR_MAP_CELLS as u64;
        let c0 = (a as u128 * n as u128 / total as u128) as usize;
        let c1 = ((b as u128 * n as u128).div_ceil(total as u128) as usize).max(c0 + 1);
        for c in &mut self.map[c0..c1.min(SECTOR_MAP_CELLS)] {
            if state == 3 || (*c != 3 && state > *c) {
                *c = state;
            }
        }
        self.map_dirty = true;
    }

    fn push_hit(&mut self, mut hit: Hit, extra: Option<HitExtra>) {
        if let Some(e) = extra {
            hit.extra = self.s.extras.len() as u32;
            self.pending.extras.push(e.clone());
            self.s.extras.push(e);
        } else {
            hit.extra = u32::MAX;
        }
        if self
            .s
            .bad_ranges
            .iter()
            .rev()
            .take(64)
            .any(|b| b.start < hit.end() && hit.offset < b.end)
        {
            hit.flags |= flags::BAD_SECTORS;
        }
        self.mark(hit.offset..hit.end(), 2);
        self.pending.hits.push(hit);
        self.s.hits.push(hit);
    }

    fn add_quick(&mut self, q: QuickFile) {
        let ext = q.name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        let offset = q
            .extents
            .iter()
            .find(|e| e.0 != u64::MAX)
            .map(|e| e.0)
            .unwrap_or(0);
        let mut f = flags::QUICK;
        if q.extents.len() > 1 {
            f |= flags::FRAGMENTED;
        }
        if q.inline.is_some() {
            f |= flags::RESIDENT;
        }
        if q.inline.is_none() {
            self.quick_starts.insert(offset);
        }
        let hit = Hit {
            offset,
            len: q.size,
            extra: 0,
            kind: kinds::lookup(ext),
            sig: u16::MAX,
            confidence: q.confidence,
            flags: f,
        };
        let extra = HitExtra {
            name: Some(q.name),
            path: Some(q.path),
            extents: if q.extents.len() > 1 {
                q.extents
            } else {
                Vec::new()
            },
            inline: q.inline,
            modified: q.modified,
            created: q.created,
            meta: None,
        };
        self.push_hit(hit, Some(extra));
    }

    fn apply_chunk(&mut self, r: ChunkResult) {
        self.bytes_done += r.own;
        let end = r.start + r.own;
        match self.s.scanned.last_mut() {
            Some(last) if last.end >= r.start && last.start <= r.start => {
                last.end = last.end.max(end)
            }
            _ => self.s.scanned.push(r.start..end),
        }
        self.mark(r.start..end, 1);
        for b in r.bad {
            let sector = 512u64;
            if b.start == self.bad_run.1 {
                self.bad_run.1 = b.end;
            } else {
                self.bad_run = (b.start, b.end);
            }
            if !self.warned_bad && (self.bad_run.1 - self.bad_run.0) / sector >= 1000 {
                self.warned_bad = true;
                let _ = self.tx.send(ScanEvent::Warning(
                    "The disk has many unreadable sectors in a row. Stop the scan and create a disk image first, then scan the image.".into(),
                ));
            }
            self.mark(b.clone(), 3);
            self.pending_bad.push(b.clone());
            self.s.bad_ranges.push(b);
        }
        for f in r.found {
            if self.quick_starts.contains(&f.start) {
                continue;
            }
            if self.covered.get(&f.sig).is_some_and(|&e| f.start < e) {
                continue;
            }
            if f.confidence >= Confidence::Medium {
                self.covered.insert(f.sig, f.start + f.len);
            }
            let hit = Hit {
                offset: f.start,
                len: f.len,
                extra: 0,
                kind: f.kind,
                sig: f.sig,
                confidence: f.confidence,
                flags: if f.truncated { flags::TRUNCATED } else { 0 },
            };
            let extra = f.meta.map(|m| HitExtra {
                meta: Some(m),
                ..Default::default()
            });
            self.push_hit(hit, extra);
        }
    }

    fn emit(&mut self, force: bool) {
        if !force && self.last_emit.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_emit = Instant::now();
        let now = Instant::now();
        self.rate.push_back((now, self.bytes_done));
        while self
            .rate
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(3))
        {
            self.rate.pop_front();
        }
        let bps = match (self.rate.front(), self.rate.back()) {
            (Some(a), Some(b)) if b.0 > a.0 => {
                (b.1 - a.1) as f64 / b.0.duration_since(a.0).as_secs_f64()
            }
            _ => 0.0,
        };
        let total = self.total();
        let eta = (bps > 1.0)
            .then(|| Duration::from_secs_f64(total.saturating_sub(self.bytes_done) as f64 / bps));
        let _ = self.tx.send(ScanEvent::Progress {
            bytes_done: self.bytes_done.min(total),
            bytes_total: total,
            bytes_per_sec: bps,
            eta,
        });
        if !self.pending.hits.is_empty() {
            let _ = self
                .tx
                .send(ScanEvent::Hits(std::mem::take(&mut self.pending)));
        }
        for b in self.pending_bad.drain(..) {
            let _ = self.tx.send(ScanEvent::BadRange(b));
        }
        if self.map_dirty {
            self.map_dirty = false;
            let _ = self.tx.send(ScanEvent::SectorMap(self.map.clone()));
        }
    }

    fn checkpoint(&mut self) {
        self.last_checkpoint = Instant::now();
        if let Some(p) = &self.checkpoint_path {
            if let Err(e) = super::save_session(p, &self.s) {
                tracing::warn!("checkpoint failed: {e}");
            }
        }
    }

    fn tick(&mut self, paused: bool) {
        self.emit(false);
        if paused && !self.was_paused {
            self.checkpoint();
        }
        self.was_paused = paused;
        if self.last_checkpoint.elapsed() >= Duration::from_secs(30) {
            self.checkpoint();
        }
    }
}

// ---------------------------------------------------------------- quick scan

struct Ctl<'a, 'b> {
    h: &'a ScanHandle,
    agg: &'a RefCell<Agg<'b>>,
    base: u64,
    span: u64,
}

impl ScanControl for Ctl<'_, '_> {
    fn should_stop(&self) -> bool {
        let paused = self.h.is_paused();
        if let Ok(mut a) = self.agg.try_borrow_mut() {
            a.tick(paused);
        }
        self.h.wait_if_paused()
    }
    fn progress(&self, fraction: f32) {
        if let Ok(mut a) = self.agg.try_borrow_mut() {
            let total = a.total();
            let done = self.base + (self.span as f64 * fraction.clamp(0.0, 1.0) as f64) as u64;
            if a.bytes_done < done.min(total) && a.s.scanned.is_empty() {
                // Quick-scan progress is shown as a share of the work, not bytes read.
                a.bytes_done = done.min(total);
            }
            a.emit(false);
        }
    }
}

fn quick_scan(src: &SourceHandle, range: &Range<u64>, h: &ScanHandle, agg: &RefCell<Agg>) {
    let parts: Vec<_> = fs::partition::detect(src)
        .into_iter()
        .filter(|p| p.offset < range.end && p.offset + p.len > range.start)
        .filter(|p| p.fs.quick_scan_supported())
        .collect();
    if parts.is_empty() {
        let _ = agg.borrow().tx.send(ScanEvent::Warning(
            "No NTFS, FAT or exFAT file system was found, so file names can't be recovered. The deep scan still finds files by their content.".into(),
        ));
    }
    let total = agg.borrow().total();
    let n = parts.len().max(1) as u64;
    for (i, p) in parts.iter().enumerate() {
        let Some(scanner) = fs::scanner_for(&p.fs) else {
            continue;
        };
        let ctl = Ctl {
            h,
            agg,
            base: total / n * i as u64,
            span: total / n,
        };
        let res = scanner.scan(src, p, &ctl, &mut |q| agg.borrow_mut().add_quick(q));
        match res {
            Ok(()) => {}
            Err(CoreError::Cancelled) => return,
            Err(e) => {
                let _ = agg.borrow().tx.send(ScanEvent::Warning(format!(
                    "Couldn't read the {} file system on partition {}: {e}",
                    p.fs.label(),
                    p.index
                )));
            }
        }
    }
    let mut a = agg.borrow_mut();
    if a.s.scanned.is_empty() {
        a.bytes_done = 0;
    }
    a.emit(true);
}

// ---------------------------------------------------------------- deep scan

struct Chunk {
    idx: u64,
    start: u64,
    own: u64,
    got: usize,
    buf: PooledBuf,
    bad: Vec<Range<u64>>,
}

struct ChunkResult {
    idx: u64,
    start: u64,
    own: u64,
    bad: Vec<Range<u64>>,
    found: Vec<Found>,
}

fn deep_scan(
    src: Arc<SourceHandle>,
    dev: &device::DeviceInfo,
    range: &Range<u64>,
    opts: &ScanOptions,
    h: &ScanHandle,
    agg: &RefCell<Agg>,
) -> Result<()> {
    let matcher = build_matcher(&opts.enabled, opts.cluster_aligned_only).ok_or_else(|| {
        CoreError::Invalid("No file types are selected. Turn some on in Settings → Formats.".into())
    })?;
    // Resolvers read through their own read-only handle.
    let resolver_src: Arc<dyn BlockRead> = match device::open_reader(dev) {
        Ok(h) => Arc::new(h),
        Err(_) => src.clone(),
    };
    let align = (src.alignment() as u64).max(512);
    let chunk = align_up((opts.chunk_size as u64).clamp(4 << 20, 64 << 20), align);
    let overlap = align_up(matcher.overlap as u64 + 1, align);
    let start = {
        let a = agg.borrow();
        let frontier =
            a.s.scanned
                .iter()
                .filter(|r| r.start <= range.start)
                .map(|r| r.end)
                .max()
                .unwrap_or(range.start);
        crate::util::align_down(frontier.max(range.start), align).max(range.start)
    };
    let end = range.end;
    if start >= end {
        return Ok(());
    }
    {
        // A resumed scan continues from its frontier.
        let mut a = agg.borrow_mut();
        a.s.scanned.retain(|r| r.start < start);
        if let Some(l) = a.s.scanned.last_mut() {
            l.end = l.end.min(start);
        }
        a.bytes_done = start - range.start;
    }
    let n_chunks = (end - start).div_ceil(chunk);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let q = if opts.queue_depth > 0 {
        opts.queue_depth.clamp(1, 16)
    } else if dev.prefers_sequential() {
        1
    } else if dev.kind == DeviceKind::ImageFile {
        2
    } else {
        4
    };
    let buf_len = (chunk + overlap) as usize;
    let count = BufferPool::sizing(buf_len, cores, device::total_ram());
    let pool = BufferPool::new(count, buf_len, align.max(4096) as usize);
    let policy = BadSectorPolicy {
        retries: opts.retries,
        ..Default::default()
    };
    let (chunk_tx, chunk_rx) = bounded::<Chunk>(count);
    let (res_tx, res_rx) = bounded::<ChunkResult>(count * 2);
    let workers = rayon::ThreadPoolBuilder::new()
        .num_threads(cores)
        .thread_name(|i| format!("ffr-match-{i}"))
        .build()
        .map_err(|e| CoreError::Invalid(e.to_string()))?;
    let size = src.size();

    std::thread::scope(|s| {
        for k in 0..q {
            let tx = chunk_tx.clone();
            let pool = pool.clone();
            let src = src.clone();
            s.spawn(move || {
                let mut idx = k as u64;
                while idx < n_chunks {
                    if h.wait_if_paused() {
                        break;
                    }
                    let Some(mut buf) = pool.get() else { break };
                    let cstart = start + idx * chunk;
                    let own = chunk.min(end - cstart);
                    let want = align_up(own + overlap, align)
                        .min(buf.len() as u64)
                        .min(size - cstart) as usize;
                    let mut bad = Vec::new();
                    let got = read_with_retry(&*src, cstart, &mut buf[..want], policy, &mut bad);
                    let c = Chunk {
                        idx,
                        start: cstart,
                        own,
                        got,
                        buf,
                        bad,
                    };
                    if tx.send(c).is_err() {
                        break;
                    }
                    idx += q as u64;
                }
            });
        }
        drop(chunk_tx);
        {
            let rx = chunk_rx.clone();
            let res_tx = res_tx.clone();
            let matcher = &matcher;
            let rsrc = resolver_src.clone();
            s.spawn(move || {
                workers.scope(|ws| {
                    for _ in 0..cores {
                        let rx = rx.clone();
                        let res_tx = res_tx.clone();
                        let rsrc = rsrc.clone();
                        ws.spawn(move |_| {
                            let mut cands = Vec::new();
                            while let Ok(c) = rx.recv() {
                                cands.clear();
                                let data = &c.buf[..c.got];
                                let found = if h.is_cancelled() {
                                    Vec::new()
                                } else {
                                    match_chunk(matcher, c.start, data, c.own as usize, &mut cands);
                                    resolve_candidates(&*rsrc, c.start, data, &mut cands)
                                };
                                let r = ChunkResult {
                                    idx: c.idx,
                                    start: c.start,
                                    own: c.own,
                                    bad: c.bad,
                                    found,
                                };
                                drop(c.buf);
                                if res_tx.send(r).is_err() {
                                    break;
                                }
                            }
                        });
                    }
                });
            });
        }
        drop(chunk_rx);
        drop(res_tx);

        let mut next = 0u64;
        let mut waiting: BTreeMap<u64, ChunkResult> = BTreeMap::new();
        loop {
            match res_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(r) => {
                    waiting.insert(r.idx, r);
                    let mut a = agg.borrow_mut();
                    while let Some(r) = waiting.remove(&next) {
                        if !h.is_cancelled() {
                            a.apply_chunk(r);
                        }
                        next += 1;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            agg.borrow_mut().tick(h.is_paused());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_and_ac_paths_agree() {
        let all_ids: BTreeSet<String> = ["jpeg", "png", "tar"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut data = vec![0u8; 8192];
        data[1024..1027].copy_from_slice(&[0xFF, 0xD8, 0xFF]);
        data[2048 + 257..2048 + 262].copy_from_slice(b"ustar");
        data[3000..3003].copy_from_slice(&[0xFF, 0xD8, 0xFF]); // unaligned
        let fast = build_matcher(&all_ids, true).unwrap();
        let slow = build_matcher(&all_ids, false).unwrap();
        let mut a = Vec::new();
        let mut b = Vec::new();
        match_chunk(&fast, 0, &data, data.len(), &mut a);
        match_chunk(&slow, 0, &data, data.len(), &mut b);
        a.sort();
        b.sort();
        assert_eq!(
            a.iter().map(|c| c.start).collect::<Vec<_>>(),
            vec![1024, 2048]
        );
        assert_eq!(
            b.iter().map(|c| c.start).collect::<Vec<_>>(),
            vec![1024, 2048, 3000]
        );
    }

    #[test]
    fn ownership_avoids_duplicates() {
        let ids: BTreeSet<String> = ["jpeg"].iter().map(|s| s.to_string()).collect();
        let m = build_matcher(&ids, false).unwrap();
        let mut data = vec![0u8; 4096];
        data[2047..2050].copy_from_slice(&[0xFF, 0xD8, 0xFF]); // straddles 2048
        let mut a = Vec::new();
        match_chunk(&m, 0, &data[..2048 + m.overlap], 2048, &mut a);
        match_chunk(&m, 2048, &data[2048..], 2048, &mut a);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].start, 2047);
    }
}
