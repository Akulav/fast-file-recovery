use super::{
    carved_name, segments, Layout, PartialPolicy, RecoverEvent, RecoverOptions, RecoverSummary,
};
use crate::device;
use crate::fs::clean_name;
use crate::io::{read_with_retry, BadSectorPolicy, BlockRead};
use crate::scan::{flags, Hit, HitExtra, HitId, Session};
use crate::JobHandle;
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Job {
    hit: Hit,
    extra: Option<HitExtra>,
    rel: PathBuf,
}

#[derive(Serialize, Clone)]
struct Row {
    name: String,
    source_offset: u64,
    length: u64,
    category: String,
    confidence: String,
    original_path: String,
    blake3: String,
    bad_sectors: bool,
    status: String,
}

fn unique(rel: PathBuf, taken: &mut HashSet<PathBuf>) -> PathBuf {
    let key = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    if taken.insert(key(&rel)) {
        return rel;
    }
    let stem = rel
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = rel.extension().map(|s| s.to_string_lossy().into_owned());
    let parent = rel.parent().map(Path::to_path_buf).unwrap_or_default();
    for n in 2..1_000_000 {
        let name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = parent.join(name);
        if taken.insert(key(&p)) {
            return p;
        }
    }
    rel
}

fn plan(session: &Session, selection: &[HitId], layout: Layout) -> Vec<Job> {
    let mut taken = HashSet::new();
    let mut jobs = Vec::with_capacity(selection.len());
    for &id in selection {
        let Some(hit) = session.hits.get(id as usize).copied() else {
            continue;
        };
        let extra = session.extra(&hit).cloned();
        let kind = hit.kind();
        let orig = extra.as_ref().and_then(|e| e.name.clone());
        let name = match &orig {
            Some(n) => clean_name(n),
            None => carved_name(&hit, extra.as_ref().and_then(|e| e.meta.as_ref())),
        };
        let ext = session.display_ext(&hit);
        let rel = match layout {
            Layout::ByType => PathBuf::from(kind.category.name())
                .join(clean_name(&ext))
                .join(&name),
            Layout::Flat => PathBuf::from(&name),
            Layout::OriginalPaths => match (&orig, extra.as_ref().and_then(|e| e.path.as_ref())) {
                (Some(_), Some(p)) => {
                    let mut pb = PathBuf::new();
                    for part in p
                        .split('/')
                        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
                    {
                        pb.push(clean_name(part));
                    }
                    pb.join(&name)
                }
                _ => PathBuf::from("_Carved").join(clean_name(&ext)).join(&name),
            },
        };
        jobs.push(Job {
            rel: unique(rel, &mut taken),
            hit,
            extra,
        });
    }
    jobs
}

/// Recover the selected hits on worker threads.
pub fn start_recovery(
    session: &Session,
    selection: Vec<HitId>,
    opts: RecoverOptions,
) -> (JobHandle, Receiver<RecoverEvent>) {
    let handle = JobHandle::default();
    let (tx, rx) = unbounded();
    let device = session.source.device.clone();
    let jobs = plan(session, &selection, opts.layout);
    let h = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("ffr-recover".into())
        .spawn(move || {
            let tx2 = tx.clone();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(device, jobs, opts, &h, &tx2)
            }));
            match r {
                Ok(Ok(s)) => {
                    let _ = tx.send(RecoverEvent::Done(s));
                }
                Ok(Err(e)) => {
                    let _ = tx.send(RecoverEvent::Failed(e));
                }
                Err(_) => {
                    let _ = tx.send(RecoverEvent::Failed(
                        "Recovery stopped because of an internal error.".into(),
                    ));
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!("could not start recovery thread: {e}");
    }
    (handle, rx)
}

fn run(
    dev: device::DeviceInfo,
    jobs: Vec<Job>,
    opts: RecoverOptions,
    h: &JobHandle,
    tx: &Sender<RecoverEvent>,
) -> Result<RecoverSummary, String> {
    let t0 = Instant::now();
    let src = device::open_reader(&dev).map_err(|e| e.to_string())?;
    let (y, mo, d, hh, mi, ss) = crate::util::now_utc();
    let folder = opts
        .dest
        .join(format!("FFR_{y:04}{mo:02}{d:02}_{hh:02}{mi:02}{ss:02}"));
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Couldn't create the output folder: {e}"))?;
    let files_total = jobs.len() as u64;
    let bytes_total: u64 = jobs.iter().map(|j| j.hit.len).sum();
    let next = AtomicUsize::new(0);
    let files_done = AtomicU64::new(0);
    let bytes_done = AtomicU64::new(0);
    let current = Mutex::new(String::new());
    let rows: Mutex<Vec<(usize, Row)>> = Mutex::new(Vec::with_capacity(jobs.len()));
    let writers = if opts.dest_rotational { 2 } else { 4 };
    let policy = BadSectorPolicy {
        retries: opts.retries,
        ..Default::default()
    };
    let active = AtomicUsize::new(writers);
    std::thread::scope(|s| {
        for _ in 0..writers {
            s.spawn(|| {
                loop {
                    if h.is_cancelled() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    if let Ok(mut c) = current.lock() {
                        *c = job.rel.to_string_lossy().into_owned();
                    }
                    let row = write_one(&src, &folder, job, &opts, policy, &bytes_done, h);
                    files_done.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut r) = rows.lock() {
                        r.push((i, row));
                    }
                }
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }
        let mut last = (Instant::now(), 0u64);
        let mut rate = 0.0f64;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let done = files_done.load(Ordering::Relaxed);
            let b = bytes_done.load(Ordering::Relaxed);
            let dt = last.0.elapsed().as_secs_f64();
            if dt >= 0.5 {
                rate = (b - last.1) as f64 / dt;
                last = (Instant::now(), b);
            }
            let _ = tx.send(RecoverEvent::Progress {
                files_done: done,
                files_total,
                bytes_done: b,
                bytes_total,
                bytes_per_sec: rate,
                current: current.lock().map(|c| c.clone()).unwrap_or_default(),
            });
            if active.load(Ordering::SeqCst) == 0 {
                break;
            }
        }
    });
    let mut rows = rows.into_inner().unwrap_or_default();
    rows.sort_by_key(|(i, _)| *i);
    let rows: Vec<Row> = rows.into_iter().map(|(_, r)| r).collect();
    let recovered = rows.iter().filter(|r| r.status == "ok").count() as u64;
    let partial = rows.iter().filter(|r| r.status == "partial").count() as u64;
    let failed = rows.len() as u64 - recovered - partial;
    let report_csv = folder.join("ffr-report.csv");
    let report_json = folder.join("ffr-report.json");
    write_csv(&report_csv, &rows).map_err(|e| format!("Couldn't write the report: {e}"))?;
    let json = serde_json::to_vec_pretty(&rows).map_err(|e| e.to_string())?;
    std::fs::write(&report_json, json).map_err(|e| format!("Couldn't write the report: {e}"))?;
    if let Some((uid, gid)) = opts.chown {
        device::chown_tree(&folder, uid, gid);
    }
    Ok(RecoverSummary {
        recovered,
        partial,
        failed,
        bytes: bytes_done.load(Ordering::Relaxed),
        folder,
        report_csv,
        report_json,
        elapsed_secs: t0.elapsed().as_secs_f64(),
        cancelled: h.is_cancelled(),
    })
}

fn write_one(
    src: &dyn BlockRead,
    folder: &Path,
    job: &Job,
    opts: &RecoverOptions,
    policy: BadSectorPolicy,
    bytes_done: &AtomicU64,
    h: &JobHandle,
) -> Row {
    let hit = &job.hit;
    let mut row = Row {
        name: job.rel.to_string_lossy().into_owned(),
        source_offset: hit.offset,
        length: hit.len,
        category: hit.kind().category.name().into(),
        confidence: hit.confidence.label().into(),
        original_path: job
            .extra
            .as_ref()
            .and_then(|e| {
                e.path
                    .as_ref()
                    .map(|p| format!("{p}/{}", e.name.clone().unwrap_or_default()))
            })
            .unwrap_or_default(),
        blake3: String::new(),
        bad_sectors: hit.has(flags::BAD_SECTORS),
        status: "ok".into(),
    };
    let path = folder.join(&job.rel);
    if let Some(p) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(p) {
            row.status = format!("failed: {e}");
            return row;
        }
    }
    let file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            row.status = format!("failed: {e}");
            return row;
        }
    };
    let mut w = BufWriter::with_capacity(1 << 20, file);
    let mut hasher = blake3::Hasher::new();
    let mut bad = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut result: std::io::Result<()> = Ok(());
    if let Some(bytes) = job.extra.as_ref().and_then(|e| e.inline.as_ref()) {
        hasher.update(bytes);
        result = w.write_all(bytes);
        bytes_done.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    } else {
        'seg: for (off, len) in segments(hit, job.extra.as_ref()) {
            let mut done = 0u64;
            while done < len {
                if h.is_cancelled() {
                    result = Err(std::io::Error::other("cancelled"));
                    break 'seg;
                }
                let n = (len - done).min(buf.len() as u64) as usize;
                if off == u64::MAX {
                    buf[..n].fill(0);
                } else {
                    let got = read_with_retry(src, off + done, &mut buf[..n], policy, &mut bad);
                    if got < n {
                        buf[got..n].fill(0);
                        bad.push(off + done + got as u64..off + done + n as u64);
                    }
                }
                hasher.update(&buf[..n]);
                if let Err(e) = w.write_all(&buf[..n]) {
                    result = Err(e);
                    break 'seg;
                }
                done += n as u64;
                bytes_done.fetch_add(n as u64, Ordering::Relaxed);
            }
        }
    }
    let flushed = w.flush().and(result);
    drop(w);
    if let Err(e) = flushed {
        row.status = if e.to_string() == "cancelled" {
            "failed: cancelled".into()
        } else {
            format!("failed: {e}")
        };
        let _ = std::fs::remove_file(&path);
        return row;
    }
    row.blake3 = hasher.finalize().to_hex().to_string();
    let partial = !bad.is_empty() || hit.has(flags::TRUNCATED);
    if !bad.is_empty() {
        row.bad_sectors = true;
    }
    let mut final_path = path.clone();
    if partial {
        row.status = "partial".into();
        if opts.partial_policy == PartialPolicy::Suffix {
            let mut p = path.clone().into_os_string();
            p.push(".partial");
            let p = PathBuf::from(p);
            if std::fs::rename(&path, &p).is_ok() {
                final_path = p;
                row.name.push_str(".partial");
            }
        }
    }
    if opts.write_timestamps {
        if let Some(m) = job.extra.as_ref().and_then(|e| e.modified) {
            if m > 0 {
                let t = UNIX_EPOCH + Duration::from_secs(m as u64);
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&final_path) {
                    let _ = f.set_modified(t.min(SystemTime::now()));
                }
            }
        }
    }
    row
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_csv(path: &Path, rows: &[Row]) -> std::io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    writeln!(
        w,
        "name,source_offset,length,category,confidence,original_path,blake3,bad_sectors,status"
    )?;
    for r in rows {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{},{}",
            csv_field(&r.name),
            r.source_offset,
            r.length,
            r.category,
            r.confidence,
            csv_field(&r.original_path),
            r.blake3,
            r.bad_sectors,
            csv_field(&r.status)
        )?;
    }
    w.flush()
}
