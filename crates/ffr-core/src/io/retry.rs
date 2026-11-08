use super::BlockRead;
use std::io;
use std::ops::Range;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct BadSectorPolicy {
    /// Extra attempts per sector-sized piece before it is declared bad.
    pub retries: u32,
    /// Base delay between attempts; grows linearly with the attempt number.
    pub delay: Duration,
}

impl Default for BadSectorPolicy {
    fn default() -> Self {
        Self {
            retries: 2,
            delay: Duration::from_millis(20),
        }
    }
}

/// Read until `buf` is full or the source ends. Returns bytes read.
pub fn read_full(src: &dyn BlockRead, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match src.read_at(offset + done as u64, &mut buf[done..]) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// Read `buf` at `offset`, never failing. If a read fails, the range is split in half
/// recursively down to one sector, each piece retried `policy.retries` times with a growing
/// delay. Unreadable sectors are zero-filled and appended to `bad`.
///
/// Returns the number of bytes covered (short only at the end of the source).
pub fn read_with_retry(
    src: &dyn BlockRead,
    offset: u64,
    buf: &mut [u8],
    policy: BadSectorPolicy,
    bad: &mut Vec<Range<u64>>,
) -> usize {
    let size = src.size();
    if offset >= size {
        return 0;
    }
    let want = buf.len().min((size - offset) as usize);
    let buf = &mut buf[..want];
    if let Ok(n) = read_full(src, offset, buf) {
        if n == want {
            return n;
        }
    }
    let sector = src.sector_size().max(512) as usize;
    split_read(src, offset, buf, sector, policy, bad);
    want
}

fn split_read(
    src: &dyn BlockRead,
    offset: u64,
    buf: &mut [u8],
    sector: usize,
    policy: BadSectorPolicy,
    bad: &mut Vec<Range<u64>>,
) {
    if buf.len() <= sector {
        for attempt in 0..=policy.retries {
            if attempt > 0 {
                std::thread::sleep(policy.delay * attempt);
            }
            if let Ok(n) = read_full(src, offset, buf) {
                if n == buf.len() {
                    return;
                }
            }
        }
        buf.fill(0);
        push_bad(bad, offset..offset + buf.len() as u64);
        return;
    }
    // Split on a sector boundary.
    let half = (buf.len() / 2).div_ceil(sector) * sector;
    let half = half.min(buf.len());
    let (a, b) = buf.split_at_mut(half);
    match read_full(src, offset, a) {
        Ok(n) if n == a.len() => {}
        _ => split_read(src, offset, a, sector, policy, bad),
    }
    let off_b = offset + half as u64;
    match read_full(src, off_b, b) {
        Ok(n) if n == b.len() => {}
        _ => split_read(src, off_b, b, sector, policy, bad),
    }
}

fn push_bad(bad: &mut Vec<Range<u64>>, r: Range<u64>) {
    if let Some(last) = bad.last_mut() {
        if last.end == r.start {
            last.end = r.end;
            return;
        }
    }
    bad.push(r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{FaultyReader, MemSource};

    #[test]
    fn bad_sectors_are_zero_filled_and_recorded() {
        let data: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8 + 1).collect();
        let r = FaultyReader {
            inner: MemSource::new(data.clone()),
            bad: vec![5000..5100, 40_000..41_000],
        };
        let mut buf = vec![0u8; data.len()];
        let mut bad = Vec::new();
        let policy = BadSectorPolicy {
            retries: 1,
            delay: Duration::from_millis(0),
        };
        let n = read_with_retry(&r, 0, &mut buf, policy, &mut bad);
        assert_eq!(n, data.len());
        assert_eq!(bad, vec![4608..5120, 39_936..41_472]);
        for (i, (&got, &want)) in buf.iter().zip(&data).enumerate() {
            let in_bad = bad.iter().any(|r| r.contains(&(i as u64)));
            assert_eq!(got, if in_bad { 0 } else { want }, "byte {i}");
        }
    }
}
