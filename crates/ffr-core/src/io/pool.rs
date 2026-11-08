use crossbeam_channel::{bounded, Receiver, Sender};
use std::ops::{Deref, DerefMut};

/// A heap buffer whose usable slice starts at an `align`-byte boundary (needed for
/// unbuffered / direct I/O). Built without `unsafe` by over-allocating.
pub struct AlignedBuf {
    raw: Vec<u8>,
    start: usize,
    len: usize,
}

impl AlignedBuf {
    pub fn new(len: usize, align: usize) -> Self {
        let align = align.max(1);
        let raw = vec![0u8; len + align];
        let start = raw.as_ptr().align_offset(align).min(align);
        Self { raw, start, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Deref for AlignedBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.raw[self.start..self.start + self.len]
    }
}

impl DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.raw[self.start..self.start + self.len]
    }
}

/// A fixed set of aligned buffers, allocated once. Getting a buffer blocks while all are in
/// use, which gives the pipeline backpressure and keeps memory at O(pool).
#[derive(Clone)]
pub struct BufferPool {
    tx: Sender<AlignedBuf>,
    rx: Receiver<AlignedBuf>,
    buf_len: usize,
}

impl BufferPool {
    pub fn new(count: usize, buf_len: usize, align: usize) -> Self {
        let count = count.max(1);
        let (tx, rx) = bounded(count);
        for _ in 0..count {
            // The channel has exactly `count` slots, so this cannot block or fail.
            let _ = tx.send(AlignedBuf::new(buf_len, align));
        }
        Self { tx, rx, buf_len }
    }

    /// Number of buffers and size so the pool stays within `2 × cores`, 1 GiB and 25 % of RAM.
    pub fn sizing(chunk: usize, cores: usize, total_ram: Option<u64>) -> usize {
        let mut cap: u64 = 1 << 30;
        if let Some(ram) = total_ram {
            cap = cap.min(ram / 4);
        }
        let by_mem = (cap / chunk.max(1) as u64).max(2) as usize;
        (2 * cores).clamp(2, by_mem.max(2))
    }

    pub fn buf_len(&self) -> usize {
        self.buf_len
    }

    /// Take a buffer, waiting for one to be returned if needed. `None` if the pool is gone.
    pub fn get(&self) -> Option<PooledBuf> {
        self.rx.recv().ok().map(|b| PooledBuf {
            buf: Some(b),
            home: self.tx.clone(),
        })
    }

    pub fn get_timeout(&self, t: std::time::Duration) -> Option<PooledBuf> {
        self.rx.recv_timeout(t).ok().map(|b| PooledBuf {
            buf: Some(b),
            home: self.tx.clone(),
        })
    }
}

/// A buffer on loan from a [`BufferPool`]; returns home when dropped.
pub struct PooledBuf {
    buf: Option<AlignedBuf>,
    home: Sender<AlignedBuf>,
}

impl Deref for PooledBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.buf.as_deref().unwrap_or(&[])
    }
}

impl DerefMut for PooledBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.buf.as_deref_mut().unwrap_or(&mut [])
    }
}

impl Drop for PooledBuf {
    fn drop(&mut self) {
        if let Some(b) = self.buf.take() {
            let _ = self.home.try_send(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned() {
        let b = AlignedBuf::new(10_000, 4096);
        assert_eq!(b.as_ptr() as usize % 4096, 0);
        assert_eq!(b.len(), 10_000);
    }

    #[test]
    fn pool_recycles() {
        let p = BufferPool::new(2, 1024, 512);
        let a = p.get().unwrap();
        let b = p.get().unwrap();
        assert!(p
            .get_timeout(std::time::Duration::from_millis(10))
            .is_none());
        drop(a);
        assert!(p
            .get_timeout(std::time::Duration::from_millis(10))
            .is_some());
        drop(b);
    }
}
