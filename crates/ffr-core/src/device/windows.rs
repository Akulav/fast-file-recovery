//! Windows device access: `\\.\PhysicalDriveN`, storage IOCTLs and volume locking.

use super::{BusType, DeviceId, DeviceInfo, DeviceKind, DiskLock, FsType, PartitionInfo, Platform};
use crate::error::{CoreError, Result};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    GetVolumePathNamesForVolumeNameW, FILE_FLAG_NO_BUFFERING, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::SystemInformation::{
    GetSystemWindowsDirectoryW, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;
const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
const FSCTL_UNLOCK_VOLUME: u32 = 0x0009_001C;
const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;

const STORAGE_DEVICE_PROPERTY: u32 = 0;
const STORAGE_ACCESS_ALIGNMENT_PROPERTY: u32 = 6;
const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
const DRIVE_REMOTE: u32 = 4;

pub(crate) struct Os;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn from_wide(b: &[u16]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..end])
}

fn share() -> u32 {
    FILE_SHARE_READ | FILE_SHARE_WRITE
}

/// Handle with no access rights, enough for query IOCTLs.
fn open_query(path: &str) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(0)
        .share_mode(share())
        .open(path)
}

fn ioctl(h: &File, code: u32, input: &[u8], out: &mut [u8]) -> io::Result<usize> {
    let mut ret: u32 = 0;
    // SAFETY: the handle is valid for the lifetime of `h`; the buffers are valid for the
    // lengths passed; no OVERLAPPED is used so the call is synchronous.
    let ok = unsafe {
        DeviceIoControl(
            h.as_raw_handle() as HANDLE,
            code,
            if input.is_empty() {
                null()
            } else {
                input.as_ptr().cast()
            },
            input.len() as u32,
            if out.is_empty() {
                null_mut()
            } else {
                out.as_mut_ptr().cast()
            },
            out.len() as u32,
            &mut ret,
            null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret as usize)
    }
}

fn query_property(h: &File, prop: u32, out: &mut [u8]) -> io::Result<usize> {
    let mut q = [0u8; 12];
    q[0..4].copy_from_slice(&prop.to_le_bytes());
    ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, out)
}

fn rd32(b: &[u8], o: usize) -> u32 {
    crate::util::le32(b, o).unwrap_or(0)
}

fn rd64(b: &[u8], o: usize) -> u64 {
    crate::util::le64(b, o).unwrap_or(0)
}

fn cstr_at(b: &[u8], off: u32) -> Option<String> {
    let off = off as usize;
    if off == 0 || off >= b.len() {
        return None;
    }
    let end = b[off..].iter().position(|&c| c == 0).map(|p| off + p)?;
    let s = String::from_utf8_lossy(&b[off..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

struct Volume {
    guid_path: String,
    letters: Vec<String>,
    disk: u32,
    offset: u64,
    len: u64,
    label: Option<String>,
    fs: FsType,
}

fn volume_extents(h: &File) -> Vec<(u32, u64, u64)> {
    let mut out = [0u8; 8 + 24 * 16];
    let Ok(n) = ioctl(h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], &mut out) else {
        return Vec::new();
    };
    let count = rd32(&out, 0) as usize;
    (0..count.min(16))
        .filter(|i| 8 + i * 24 + 24 <= n)
        .map(|i| {
            let b = 8 + i * 24;
            (rd32(&out, b), rd64(&out, b + 8), rd64(&out, b + 16))
        })
        .collect()
}

fn list_volumes() -> Vec<Volume> {
    let mut vols = Vec::new();
    let mut name = [0u16; 512];
    // SAFETY: `name` is a valid writable buffer of the given length.
    let find = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
    if find == INVALID_HANDLE_VALUE {
        return vols;
    }
    loop {
        let guid = from_wide(&name);
        let trimmed = guid.trim_end_matches('\\').to_string();
        let mut letters = Vec::new();
        let mut paths = [0u16; 1024];
        let mut ret = 0u32;
        let gw = wide(std::ffi::OsStr::new(&guid));
        // SAFETY: valid NUL-terminated input and writable output buffer with its length.
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                gw.as_ptr(),
                paths.as_mut_ptr(),
                paths.len() as u32,
                &mut ret,
            )
        };
        if ok != 0 {
            for p in paths[..(ret as usize).min(paths.len())].split(|&c| c == 0) {
                let s = String::from_utf16_lossy(p);
                if !s.is_empty() {
                    letters.push(s.trim_end_matches('\\').to_string());
                }
            }
        }
        let mut label_buf = [0u16; 261];
        let mut fs_buf = [0u16; 261];
        // SAFETY: valid NUL-terminated root path and writable buffers with their lengths.
        let ok = unsafe {
            GetVolumeInformationW(
                gw.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as u32,
                null_mut(),
                null_mut(),
                null_mut(),
                fs_buf.as_mut_ptr(),
                fs_buf.len() as u32,
            )
        };
        let (label, fs) = if ok != 0 {
            let l = from_wide(&label_buf);
            (
                (!l.is_empty()).then_some(l),
                FsType::from_name(&from_wide(&fs_buf)),
            )
        } else {
            (None, FsType::Unknown)
        };
        if let Ok(h) = open_query(&trimmed) {
            for (disk, offset, len) in volume_extents(&h) {
                vols.push(Volume {
                    guid_path: trimmed.clone(),
                    letters: letters.clone(),
                    disk,
                    offset,
                    len,
                    label: label.clone(),
                    fs: fs.clone(),
                });
            }
        }
        // SAFETY: `find` is a valid search handle and `name` a writable buffer.
        if unsafe { FindNextVolumeW(find, name.as_mut_ptr(), name.len() as u32) } == 0 {
            break;
        }
    }
    // SAFETY: `find` came from FindFirstVolumeW and is closed exactly once.
    unsafe { FindVolumeClose(find) };
    vols
}

fn system_disk(vols: &[Volume]) -> Option<u32> {
    let mut buf = [0u16; 260];
    // SAFETY: writable buffer with its length.
    let n = unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        return None;
    }
    let dir = from_wide(&buf);
    let letter = dir.get(..2)?.to_ascii_uppercase();
    vols.iter()
        .find(|v| v.letters.iter().any(|l| l.to_ascii_uppercase() == letter))
        .map(|v| v.disk)
}

fn bus_from(code: u32) -> BusType {
    match code {
        17 => BusType::Nvme,
        3 | 11 => BusType::Sata,
        7 => BusType::Usb,
        12 | 13 => BusType::Sd,
        14 | 15 => BusType::Virtual,
        _ => BusType::Unknown,
    }
}

fn disk_number(id: &DeviceId) -> Option<u32> {
    id.0.strip_prefix("PhysicalDrive")?.parse().ok()
}

impl Platform for Os {
    fn list_devices() -> Vec<DeviceInfo> {
        let vols = list_volumes();
        let sys = system_disk(&vols);
        let mut out = Vec::new();
        for n in 0..32u32 {
            let path = format!(r"\\.\PhysicalDrive{n}");
            let Ok(h) = open_query(&path) else { continue };
            let mut desc = [0u8; 1024];
            let (mut model, mut serial, mut bus, mut removable) =
                (format!("Disk {n}"), None, BusType::Unknown, false);
            if query_property(&h, STORAGE_DEVICE_PROPERTY, &mut desc).is_ok() {
                let vendor = cstr_at(&desc, rd32(&desc, 12));
                let product = cstr_at(&desc, rd32(&desc, 16));
                model = match (vendor, product) {
                    (Some(v), Some(p)) if !p.starts_with(&v) => format!("{v} {p}"),
                    (_, Some(p)) => p,
                    (Some(v), None) => v,
                    _ => model,
                };
                serial = cstr_at(&desc, rd32(&desc, 24));
                bus = bus_from(rd32(&desc, 28));
                removable = desc[10] != 0;
            }
            let mut geo = [0u8; 256];
            let (size, sector) = match ioctl(&h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geo) {
                Ok(_) => (rd64(&geo, 24), rd32(&geo, 20).max(512)),
                Err(_) => continue,
            };
            if size == 0 {
                continue;
            }
            let mut align = [0u8; 64];
            let phys = match query_property(&h, STORAGE_ACCESS_ALIGNMENT_PROPERTY, &mut align) {
                Ok(n) if n >= 24 => rd32(&align, 20).max(sector),
                _ => sector,
            };
            let mut seek = [0u8; 16];
            let rotational =
                match query_property(&h, STORAGE_DEVICE_SEEK_PENALTY_PROPERTY, &mut seek) {
                    Ok(n) if n >= 9 => Some(seek[8] != 0),
                    _ => None,
                };
            let readable = OpenOptions::new()
                .read(true)
                .share_mode(share())
                .open(&path)
                .is_ok();
            let mut partitions: Vec<PartitionInfo> = Vec::new();
            for v in vols.iter().filter(|v| v.disk == n) {
                partitions.push(PartitionInfo {
                    index: partitions.len() as u32 + 1,
                    offset: v.offset,
                    len: v.len,
                    fs: v.fs.clone(),
                    label: v.label.clone(),
                    mount_points: v.letters.clone(),
                });
            }
            partitions.sort_by_key(|p| p.offset);
            out.push(DeviceInfo {
                id: DeviceId(format!("PhysicalDrive{n}")),
                path: path.into(),
                kind: DeviceKind::PhysicalDisk,
                model,
                serial,
                bus,
                size,
                sector_size: sector,
                physical_sector_size: phys,
                rotational,
                removable: removable || bus == BusType::Usb || bus == BusType::Sd,
                is_system: sys == Some(n),
                partitions,
                readable,
            });
        }
        out
    }

    fn open_raw(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .share_mode(share())
            .custom_flags(FILE_FLAG_NO_BUFFERING)
            .open(path)
    }

    fn device_size(f: &File) -> io::Result<u64> {
        let mut out = [0u8; 8];
        ioctl(f, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut out)?;
        Ok(u64::from_le_bytes(out))
    }

    fn pread(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        f.seek_read(buf, offset)
    }

    fn lock(dev: &DeviceInfo, _unmount: bool) -> Result<Box<dyn DiskLock>> {
        let Some(n) = disk_number(&dev.id) else {
            return Ok(Box::new(WinLock::default()));
        };
        let mut lock = WinLock::default();
        let mut failed = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for v in list_volumes().into_iter().filter(|v| v.disk == n) {
            if !seen.insert(v.guid_path.clone()) {
                continue;
            }
            let name = v
                .letters
                .first()
                .cloned()
                .unwrap_or_else(|| v.label.clone().unwrap_or_else(|| "hidden volume".into()));
            let h = match OpenOptions::new()
                .read(true)
                .share_mode(share())
                .open(&v.guid_path)
            {
                Ok(h) => h,
                Err(_) => {
                    failed.push(name);
                    continue;
                }
            };
            let mut locked = false;
            for attempt in 0..3 {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                if ioctl(&h, FSCTL_LOCK_VOLUME, &[], &mut []).is_ok() {
                    locked = true;
                    break;
                }
            }
            if !locked {
                failed.push(name);
                continue;
            }
            let _ = ioctl(&h, FSCTL_DISMOUNT_VOLUME, &[], &mut []);
            lock.names.push(name);
            lock.handles.push(h);
        }
        if !failed.is_empty() {
            return Err(CoreError::LockFailed(failed));
        }
        Ok(Box::new(lock))
    }

    fn disks_of_path(path: &Path) -> Vec<DeviceId> {
        let p = wide(path.as_os_str());
        let mut mount = [0u16; 1024];
        // SAFETY: valid NUL-terminated input and writable buffer with its length.
        if unsafe { GetVolumePathNameW(p.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
            return Vec::new();
        }
        let mut vol = [0u16; 128];
        // SAFETY: `mount` is NUL-terminated (written by the OS); `vol` is writable.
        if unsafe {
            GetVolumeNameForVolumeMountPointW(mount.as_ptr(), vol.as_mut_ptr(), vol.len() as u32)
        } == 0
        {
            return Vec::new();
        }
        let v = from_wide(&vol);
        let Ok(h) = open_query(v.trim_end_matches('\\')) else {
            return Vec::new();
        };
        let mut ids: Vec<DeviceId> = volume_extents(&h)
            .into_iter()
            .map(|(d, _, _)| DeviceId(format!("PhysicalDrive{d}")))
            .collect();
        ids.dedup();
        ids
    }

    fn is_network_path(path: &Path) -> bool {
        let s = path.to_string_lossy();
        if s.starts_with(r"\\") && !s.starts_with(r"\\?\") && !s.starts_with(r"\\.\") {
            return true;
        }
        if s.starts_with(r"\\?\UNC\") {
            return true;
        }
        let p = wide(path.as_os_str());
        let mut mount = [0u16; 1024];
        // SAFETY: valid NUL-terminated input and writable buffer with its length.
        if unsafe { GetVolumePathNameW(p.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
            return false;
        }
        // SAFETY: `mount` is NUL-terminated.
        unsafe { GetDriveTypeW(mount.as_ptr()) == DRIVE_REMOTE }
    }

    fn free_space(path: &Path) -> Option<u64> {
        let p = wide(path.as_os_str());
        let mut free: u64 = 0;
        // SAFETY: valid NUL-terminated path; the out pointer is a valid u64.
        let ok = unsafe { GetDiskFreeSpaceExW(p.as_ptr(), &mut free, null_mut(), null_mut()) };
        (ok != 0).then_some(free)
    }

    fn total_ram() -> Option<u64> {
        // SAFETY: MEMORYSTATUSEX is plain data; dwLength is set as the API requires.
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `m` is a valid, initialised struct.
        let ok = unsafe { GlobalMemoryStatusEx(&mut m) };
        (ok != 0).then_some(m.ullTotalPhys)
    }

    fn chown(_path: &Path, _uid: u32, _gid: u32) {}
}

#[derive(Default)]
struct WinLock {
    handles: Vec<File>,
    names: Vec<String>,
}

impl DiskLock for WinLock {
    fn describe(&self) -> String {
        if self.names.is_empty() {
            "no mounted volumes".into()
        } else {
            self.names.join(", ")
        }
    }
}

impl Drop for WinLock {
    fn drop(&mut self) {
        for h in &self.handles {
            let _ = ioctl(h, FSCTL_UNLOCK_VOLUME, &[], &mut []);
        }
        // Closing the handles (when `handles` drops) also releases the locks; the OS remounts
        // dismounted volumes on next access.
    }
}
