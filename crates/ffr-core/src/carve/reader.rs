use crate::io::{read_full, AlignedBuf, BlockRead};
use std::time::{Duration, Instant};

const BLOCK: u64 = 1 << 20;
const CACHE_BLOCKS: usize = 4;

/// Bounded, cached reader that resolvers use to look past the current chunk.
///
/// Reads inside `window` (the chunk being matched) are free. Other reads go through a small
/// LRU of 1 MiB blocks. Every resolver gets a byte budget and a deadline so hostile data can
/// never stall the scan.
pub struct Reader<'a> {
    src: &'a dyn BlockRead,
    size: u64,
    win_start: u64,
    win: &'a [u8],
    cache: Vec<(u64, AlignedBuf, usize)>,
    budget: u64,
    deadline: Instant,
    eof: bool,
}

impl<'a> Reader<'a> {
    /// Whether a read hit the end of the device since the last call (and clear the flag).
    pub fn take_eof(&mut self) -> bool {
        std::mem::take(&mut self.eof)
    }

    pub fn new(src: &'a dyn BlockRead, win_start: u64, win: &'a [u8]) -> Self {
        Self {
            size: src.size(),
            src,
            win_start,
            win,
            cache: Vec::with_capacity(CACHE_BLOCKS),
            budget: 512 << 20,
            deadline: Instant::now() + Duration::from_secs(4),
            eof: false,
        }
    }

    /// Reset the budget and deadline for the next resolver call (the cache is kept).
    pub fn reset_limits(&mut self, budget: u64, time: Duration) {
        self.budget = budget;
        self.deadline = Instant::now() + time;
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn over_limit(&self) -> bool {
        self.budget == 0 || Instant::now() > self.deadline
    }

    fn block(&mut self, bi: u64) -> Option<&[u8]> {
        if let Some(p) = self.cache.iter().position(|(i, _, _)| *i == bi) {
            if p != 0 {
                let e = self.cache.remove(p);
                self.cache.insert(0, e);
            }
            let (_, b, n) = &self.cache[0];
            return Some(&b[..*n]);
        }
        if self.over_limit() {
            return None;
        }
        let start = bi * BLOCK;
        if start >= self.size {
            return None;
        }
        let mut buf = if self.cache.len() >= CACHE_BLOCKS {
            self.cache.pop().map(|(_, b, _)| b)?
        } else {
            AlignedBuf::new(BLOCK as usize, 4096)
        };
        let want = ((self.size - start).min(BLOCK)) as usize;
        let n = read_full(self.src, start, &mut buf[..want]).ok()?;
        self.budget = self.budget.saturating_sub(n as u64);
        self.cache.insert(0, (bi, buf, n));
        let (_, b, n) = &self.cache[0];
        Some(&b[..*n])
    }

    /// Fill `buf` from `off`. Returns false if any byte is unavailable.
    pub fn read(&mut self, off: u64, buf: &mut [u8]) -> bool {
        let Some(end) = off.checked_add(buf.len() as u64) else {
            return false;
        };
        if end > self.size {
            self.eof = true;
            return false;
        }
        let mut done = 0usize;
        while done < buf.len() {
            let pos = off + done as u64;
            let win_end = self.win_start + self.win.len() as u64;
            if pos >= self.win_start && pos < win_end {
                let i = (pos - self.win_start) as usize;
                let n = (buf.len() - done).min(self.win.len() - i);
                buf[done..done + n].copy_from_slice(&self.win[i..i + n]);
                done += n;
                continue;
            }
            let bi = pos / BLOCK;
            let i = (pos - bi * BLOCK) as usize;
            let need = buf.len() - done;
            let Some(block) = self.block(bi) else {
                return false;
            };
            if i >= block.len() {
                return false;
            }
            let n = need.min(block.len() - i);
            buf[done..done + n].copy_from_slice(&block[i..i + n]);
            done += n;
        }
        true
    }

    pub fn bytes<const N: usize>(&mut self, off: u64) -> Option<[u8; N]> {
        let mut a = [0u8; N];
        self.read(off, &mut a).then_some(a)
    }

    pub fn vec(&mut self, off: u64, len: usize) -> Option<Vec<u8>> {
        let mut v = vec![0u8; len];
        self.read(off, &mut v).then_some(v)
    }

    pub fn u8(&mut self, off: u64) -> Option<u8> {
        self.bytes::<1>(off).map(|a| a[0])
    }
    pub fn u16le(&mut self, off: u64) -> Option<u16> {
        self.bytes(off).map(u16::from_le_bytes)
    }
    pub fn u16be(&mut self, off: u64) -> Option<u16> {
        self.bytes(off).map(u16::from_be_bytes)
    }
    pub fn u32le(&mut self, off: u64) -> Option<u32> {
        self.bytes(off).map(u32::from_le_bytes)
    }
    pub fn u32be(&mut self, off: u64) -> Option<u32> {
        self.bytes(off).map(u32::from_be_bytes)
    }
    pub fn u64le(&mut self, off: u64) -> Option<u64> {
        self.bytes(off).map(u64::from_le_bytes)
    }
    pub fn u64be(&mut self, off: u64) -> Option<u64> {
        self.bytes(off).map(u64::from_be_bytes)
    }

    /// Call `f(piece_start, piece)` over `[from, from + limit)` in pieces that overlap by
    /// `overlap` bytes. Stops when `f` returns false, at the end of the device, or at a limit.
    pub fn scan(
        &mut self,
        from: u64,
        limit: u64,
        overlap: usize,
        mut f: impl FnMut(u64, &[u8]) -> bool,
    ) {
        const PIECE: usize = 256 * 1024;
        if from.saturating_add(limit) > self.size {
            // Running to the end of the device counts as hitting it.
            self.eof = true;
        }
        let end = from.saturating_add(limit).min(self.size);
        let mut pos = from;
        let mut buf = vec![0u8; PIECE + overlap];
        while pos < end {
            if self.over_limit() {
                return;
            }
            let n = ((end - pos) as usize).min(PIECE + overlap);
            if !self.read(pos, &mut buf[..n]) {
                return;
            }
            if !f(pos, &buf[..n]) {
                return;
            }
            if n <= overlap {
                return;
            }
            pos += (n - overlap) as u64;
        }
    }

    /// First occurrence of `needle` in `[from, from + limit)`.
    pub fn find(&mut self, from: u64, limit: u64, needle: &[u8]) -> Option<u64> {
        let finder = memchr::memmem::Finder::new(needle);
        let mut found = None;
        self.scan(from, limit, needle.len().saturating_sub(1), |at, piece| {
            if let Some(i) = finder.find(piece) {
                found = Some(at + i as u64);
                return false;
            }
            true
        });
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::MemSource;

    #[test]
    fn reads_across_window_and_blocks() {
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 253) as u8).collect();
        let src = MemSource::new(data.clone());
        let mut r = Reader::new(&src, 100, &data[100..200]);
        let mut b = vec![0u8; 2_000_000];
        assert!(r.read(50, &mut b));
        assert_eq!(&b[..], &data[50..2_000_050]);
        assert!(!r.read(2_999_999, &mut [0u8; 2]));
        assert_eq!(r.find(0, 3_000_000, &[10, 11, 12]), Some(10));
        // 1_048_575 % 253 == 143; the needle straddles the 1 MiB block boundary.
        assert_eq!(r.find(1_048_570, 100, &[143, 144, 145]), Some(1_048_575));
    }
}
