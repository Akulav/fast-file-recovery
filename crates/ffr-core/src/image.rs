//! Disk image creation (dd-style, bad-sector aware). Writes `<name>.img` plus
//! `<name>.img.ffrmap` listing the unreadable ranges.

use crate::device::{self, DeviceKind};
use crate::io::{read_with_retry, BadSectorPolicy};
use crate::recover::same_disk;
use crate::scan::SourceSpec;
use crate::JobHandle;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ImageOptions {
    pub retries: u32,
    pub chunk_size: usize,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            retries: 2,
            chunk_size: 4 << 20,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ImageEvent {
    Progress {
        bytes_done: u64,
        bytes_total: u64,
        bytes_per_sec: f64,
        bad_bytes: u64,
    },
    Done {
        path: PathBuf,
        map_path: PathBuf,
        bad_ranges: usize,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

/// Copy the source (or its range) to `dest` (an `.img` file on another disk).
pub fn start_imaging(
    source: SourceSpec,
    dest: PathBuf,
    opts: ImageOptions,
) -> (JobHandle, Receiver<ImageEvent>) {
    let handle = JobHandle::default();
    let (tx, rx) = unbounded();
    let h = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("ffr-image".into())
        .spawn(move || {
            let tx2 = tx.clone();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(source, dest, opts, &h, &tx2)
            }));
            let ev = match r {
                Ok(Ok(ev)) => ev,
                Ok(Err(e)) => ImageEvent::Failed(e),
                Err(_) => {
                    ImageEvent::Failed("Imaging stopped because of an internal error.".into())
                }
            };
            let _ = tx.send(ev);
        });
    if let Err(e) = spawned {
        tracing::error!("could not start imaging thread: {e}");
    }
    (handle, rx)
}

fn run(
    source: SourceSpec,
    dest: PathBuf,
    opts: ImageOptions,
    h: &JobHandle,
    tx: &Sender<ImageEvent>,
) -> Result<ImageEvent, String> {
    let t0 = Instant::now();
    let dev = &source.device;
    let dest_dir = dest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let canon_dir = std::fs::canonicalize(&dest_dir)
        .map_err(|_| "The destination folder doesn't exist.".to_string())?;
    if dev.kind == DeviceKind::ImageFile {
        let target = canon_dir.join(dest.file_name().unwrap_or_default());
        if crate::recover::same_path(&target, &dev.path) {
            return Err("The image can't overwrite its own source.".into());
        }
    } else {
        let disks = device::disks_of_path(&canon_dir);
        if disks.is_empty() || same_disk(dev, &disks) {
            return Err("Save the image on a different disk than the one you are copying.".into());
        }
    }
    let src = device::open_reader(dev).map_err(|e| e.to_string())?;
    let range: Range<u64> = source.range.clone().unwrap_or(0..src.size());
    let total = range.end.saturating_sub(range.start);
    if let Some(free) = device::free_space(&canon_dir) {
        if free < total {
            return Err(format!(
                "Not enough free space: {} needed, {} free.",
                crate::util::human_size(total),
                crate::util::human_size(free)
            ));
        }
    }
    let file =
        std::fs::File::create(&dest).map_err(|e| format!("Couldn't create the image: {e}"))?;
    let mut w = BufWriter::with_capacity(8 << 20, file);
    let policy = BadSectorPolicy {
        retries: opts.retries,
        ..Default::default()
    };
    let chunk = opts.chunk_size.clamp(1 << 20, 64 << 20);
    let mut buf = crate::io::AlignedBuf::new(chunk, src.alignment().max(4096));
    let mut bad: Vec<Range<u64>> = Vec::new();
    let mut pos = range.start;
    let mut last = Instant::now();
    let mut last_bytes = 0u64;
    let mut rate = 0.0;
    while pos < range.end {
        if h.is_cancelled() {
            break;
        }
        let n = ((range.end - pos) as usize).min(chunk);
        let got = read_with_retry(&src, pos, &mut buf[..n], policy, &mut bad);
        if got < n {
            buf[got..n].fill(0);
        }
        w.write_all(&buf[..n])
            .map_err(|e| format!("Writing the image failed: {e}"))?;
        pos += n as u64;
        if last.elapsed() >= Duration::from_millis(100) {
            let done = pos - range.start;
            rate = (done - last_bytes) as f64 / last.elapsed().as_secs_f64();
            last = Instant::now();
            last_bytes = done;
            let _ = tx.send(ImageEvent::Progress {
                bytes_done: done,
                bytes_total: total,
                bytes_per_sec: rate,
                bad_bytes: bad.iter().map(|r| r.end - r.start).sum(),
            });
        }
    }
    let _ = rate;
    w.flush()
        .map_err(|e| format!("Writing the image failed: {e}"))?;
    let mut map_path = dest.clone().into_os_string();
    map_path.push(".ffrmap");
    let map_path = PathBuf::from(map_path);
    let mut map = String::from("# Fast File Recovery bad-sector map\n");
    map.push_str(&format!("# source: {} ({})\n", dev.title(), dev.id));
    map.push_str(&format!("# range: {}-{}\n", range.start, range.end));
    map.push_str("# unreadable byte ranges, relative to the image start:\n");
    for r in &bad {
        map.push_str(&format!(
            "{}-{}\n",
            r.start - range.start,
            r.end - range.start
        ));
    }
    std::fs::write(&map_path, map).map_err(|e| format!("Couldn't write the map: {e}"))?;
    Ok(ImageEvent::Done {
        path: dest,
        map_path,
        bad_ranges: bad.len(),
        elapsed: t0.elapsed(),
        cancelled: h.is_cancelled(),
    })
}
