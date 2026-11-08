//! Aligned buffers, the buffer pool, and bad-sector-tolerant reading.

mod pool;
mod retry;

pub use pool::{AlignedBuf, BufferPool, PooledBuf};
pub use retry::{read_full, read_with_retry, BadSectorPolicy};

use std::io;

/// Anything the engine can read from: a device handle, an image file, or a test double.
///
/// There is deliberately no write method.
pub trait BlockRead: Send + Sync {
    /// Read up to `buf.len()` bytes at `offset`. Returns the number of bytes read; 0 at the end.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
    /// Total readable size in bytes.
    fn size(&self) -> u64;
    /// Logical sector size (the smallest unit a bad-sector split goes down to).
    fn sector_size(&self) -> u32;
}

impl<T: BlockRead + ?Sized> BlockRead for std::sync::Arc<T> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        (**self).read_at(offset, buf)
    }
    fn size(&self) -> u64 {
        (**self).size()
    }
    fn sector_size(&self) -> u32 {
        (**self).sector_size()
    }
}

/// An in-memory block source (used for tests and for small images).
pub struct MemSource {
    pub data: Vec<u8>,
    pub sector: u32,
}

impl MemSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, sector: 512 }
    }
}

impl BlockRead for MemSource {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let len = self.data.len() as u64;
        if offset >= len {
            return Ok(0);
        }
        let n = buf.len().min((len - offset) as usize);
        buf[..n].copy_from_slice(&self.data[offset as usize..offset as usize + n]);
        Ok(n)
    }
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn sector_size(&self) -> u32 {
        self.sector
    }
}

/// Test double that fails reads overlapping configured ranges.
pub struct FaultyReader<R: BlockRead> {
    pub inner: R,
    pub bad: Vec<std::ops::Range<u64>>,
}

impl<R: BlockRead> BlockRead for FaultyReader<R> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let end = offset + buf.len() as u64;
        if self.bad.iter().any(|r| r.start < end && offset < r.end) {
            return Err(io::Error::other("simulated bad sector"));
        }
        self.inner.read_at(offset, buf)
    }
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn sector_size(&self) -> u32 {
        self.inner.sector_size()
    }
}
