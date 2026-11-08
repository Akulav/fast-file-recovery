//! Fallback for unsupported platforms: only disk image files work.

use super::{DeviceId, DeviceInfo, DiskLock, Platform};
use crate::error::Result;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub(crate) struct Os;

struct NoLock;
impl DiskLock for NoLock {
    fn describe(&self) -> String {
        "not locked".into()
    }
}

impl Platform for Os {
    fn list_devices() -> Vec<DeviceInfo> {
        Vec::new()
    }
    fn open_raw(path: &Path) -> io::Result<File> {
        File::open(path)
    }
    fn device_size(f: &File) -> io::Result<u64> {
        Ok(f.metadata()?.len())
    }
    fn pread(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let mut f = f.try_clone()?;
        f.seek(SeekFrom::Start(offset))?;
        f.read(buf)
    }
    fn lock(_dev: &DeviceInfo, _unmount: bool) -> Result<Box<dyn DiskLock>> {
        Ok(Box::new(NoLock))
    }
    fn disks_of_path(_path: &Path) -> Vec<DeviceId> {
        Vec::new()
    }
    fn is_network_path(_path: &Path) -> bool {
        false
    }
    fn free_space(_path: &Path) -> Option<u64> {
        None
    }
    fn total_ram() -> Option<u64> {
        None
    }
    fn chown(_path: &Path, _uid: u32, _gid: u32) {}
}
