//! macOS device access: `diskutil -plist`, `/dev/rdiskN` with `F_NOCACHE`, unmount as lock.

use super::{BusType, DeviceId, DeviceInfo, DeviceKind, DiskLock, FsType, PartitionInfo, Platform};
use crate::error::{CoreError, Result};
use plist::Value;
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

const DKIOCGETBLOCKSIZE: u64 = 0x4004_6418;
const DKIOCGETBLOCKCOUNT: u64 = 0x4008_6419;

pub(crate) struct Os;

fn diskutil(args: &[&str]) -> Option<Value> {
    let out = Command::new("/usr/sbin/diskutil")
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Value::from_reader_xml(&out.stdout[..]).ok()
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.as_dictionary()?.get(k)?.as_string().map(str::to_string)
}

fn n(v: &Value, k: &str) -> Option<u64> {
    v.as_dictionary()?.get(k)?.as_unsigned_integer()
}

fn b(v: &Value, k: &str) -> Option<bool> {
    v.as_dictionary()?.get(k)?.as_boolean()
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.as_dictionary()
        .and_then(|d| d.get(k))
        .and_then(|a| a.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// Map APFS container disks (synthesized) to the physical partition that stores them.
fn container_stores(list: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for d in arr(list, "AllDisksAndPartitions") {
        let Some(id) = s(d, "DeviceIdentifier") else {
            continue;
        };
        for store in arr(d, "APFSPhysicalStores") {
            if let Some(p) = s(store, "DeviceIdentifier") {
                map.insert(id.clone(), p);
            }
        }
    }
    map
}

fn whole_of(id: &str) -> String {
    // disk3s1s1 -> disk3
    let rest = id.trim_start_matches("/dev/").trim_start_matches('r');
    let digits: String = rest
        .trim_start_matches("disk")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    format!("disk{digits}")
}

fn physical_disk_for(id: &str, stores: &HashMap<String, String>) -> String {
    let mut whole = whole_of(id);
    for _ in 0..4 {
        match stores.get(&whole) {
            Some(p) => whole = whole_of(p),
            None => break,
        }
    }
    whole
}

impl Platform for Os {
    fn list_devices() -> Vec<DeviceInfo> {
        let Some(list) = diskutil(&["list", "-plist"]) else {
            return Vec::new();
        };
        let stores = container_stores(&list);
        let root_disk = diskutil(&["info", "-plist", "/"])
            .and_then(|v| s(&v, "DeviceIdentifier"))
            .map(|id| physical_disk_for(&id, &stores));
        // APFS volume mount points, keyed by the physical partition that stores them.
        let mut apfs_mounts: HashMap<String, (Vec<String>, Option<String>)> = HashMap::new();
        for d in arr(&list, "AllDisksAndPartitions") {
            let Some(id) = s(d, "DeviceIdentifier") else {
                continue;
            };
            if let Some(store) = stores.get(&id) {
                let e = apfs_mounts.entry(store.clone()).or_default();
                for v in arr(d, "APFSVolumes") {
                    if let Some(m) = s(v, "MountPoint") {
                        e.0.push(m);
                    }
                    if e.1.is_none() {
                        e.1 = s(v, "VolumeName");
                    }
                }
            }
        }
        let mut out = Vec::new();
        for d in arr(&list, "AllDisksAndPartitions") {
            let Some(id) = s(d, "DeviceIdentifier") else {
                continue;
            };
            if stores.contains_key(&id) {
                continue; // synthesized APFS container
            }
            let Some(info) = diskutil(&["info", "-plist", &id]) else {
                continue;
            };
            if s(&info, "VirtualOrPhysical").as_deref() == Some("Virtual") {
                continue;
            }
            let size = n(&info, "TotalSize")
                .or_else(|| n(&info, "Size"))
                .unwrap_or(0);
            if size == 0 {
                continue;
            }
            let proto = s(&info, "BusProtocol").unwrap_or_default();
            let bus = match proto.as_str() {
                "USB" => BusType::Usb,
                "PCI-Express" | "Apple Fabric" | "NVMe" => BusType::Nvme,
                "SATA" => BusType::Sata,
                "Secure Digital" => BusType::Sd,
                "Disk Image" | "Virtual Interface" => BusType::Virtual,
                _ => BusType::Unknown,
            };
            let sector = n(&info, "DeviceBlockSize").unwrap_or(512) as u32;
            let mut partitions = Vec::new();
            for (i, p) in arr(d, "Partitions").iter().enumerate() {
                let pid = s(p, "DeviceIdentifier").unwrap_or_default();
                let mut mounts: Vec<String> = s(p, "MountPoint").into_iter().collect();
                let mut label = s(p, "VolumeName");
                if let Some((m, l)) = apfs_mounts.get(&pid) {
                    mounts.extend(m.iter().cloned());
                    if label.is_none() {
                        label = l.clone();
                    }
                }
                let content = s(p, "Content").unwrap_or_default();
                let fs = match content.as_str() {
                    "Apple_APFS" => FsType::Apfs,
                    "Apple_HFS" => FsType::HfsPlus,
                    "Microsoft Basic Data" | "Windows_NTFS" => FsType::Unknown,
                    "DOS_FAT_32" => FsType::Fat32,
                    _ => FsType::Unknown,
                };
                partitions.push(PartitionInfo {
                    index: i as u32 + 1,
                    offset: 0,
                    len: n(p, "Size").unwrap_or(0),
                    fs,
                    label,
                    mount_points: mounts,
                });
            }
            let raw = PathBuf::from(format!("/dev/r{id}"));
            let readable = File::open(&raw).is_ok();
            out.push(DeviceInfo {
                id: DeviceId(format!("/dev/{id}")),
                path: raw,
                kind: DeviceKind::PhysicalDisk,
                model: s(&info, "MediaName")
                    .or_else(|| s(&info, "IORegistryEntryName"))
                    .unwrap_or_else(|| id.clone()),
                serial: None,
                bus,
                size,
                sector_size: sector.max(512),
                physical_sector_size: sector.max(512),
                rotational: b(&info, "SolidState").map(|ssd| !ssd),
                removable: b(&info, "Removable").unwrap_or(false)
                    || b(&info, "RemovableMedia").unwrap_or(false)
                    || b(&info, "Ejectable").unwrap_or(false),
                is_system: root_disk.as_deref() == Some(id.as_str()),
                partitions,
                readable,
            });
        }
        out
    }

    fn open_raw(path: &Path) -> io::Result<File> {
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(path)?;
        // SAFETY: valid fd; F_NOCACHE takes an int argument.
        unsafe { libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1) };
        Ok(f)
    }

    fn device_size(f: &File) -> io::Result<u64> {
        let mut bs: u32 = 0;
        let mut count: u64 = 0;
        // SAFETY: valid fd; the ioctls write a u32 and a u64 into the provided pointers.
        let r1 = unsafe { libc::ioctl(f.as_raw_fd(), DKIOCGETBLOCKSIZE as _, &mut bs as *mut u32) };
        // SAFETY: as above.
        let r2 = unsafe {
            libc::ioctl(
                f.as_raw_fd(),
                DKIOCGETBLOCKCOUNT as _,
                &mut count as *mut u64,
            )
        };
        if r1 < 0 || r2 < 0 {
            return Ok(f.metadata()?.len());
        }
        Ok(bs as u64 * count)
    }

    fn pread(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        f.read_at(buf, offset)
    }

    fn lock(dev: &DeviceInfo, unmount: bool) -> Result<Box<dyn DiskLock>> {
        if !unmount {
            return Ok(Box::new(MacLock {
                disk: None,
                name: dev.id.0.clone(),
            }));
        }
        let ok = Command::new("/usr/sbin/diskutil")
            .args(["unmountDisk", &dev.id.0])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Err(CoreError::LockFailed(vec![dev.id.0.clone()]));
        }
        Ok(Box::new(MacLock {
            disk: Some(dev.id.0.clone()),
            name: dev.id.0.clone(),
        }))
    }

    fn disks_of_path(path: &Path) -> Vec<DeviceId> {
        let Some(from) = mount_from(path) else {
            return Vec::new();
        };
        let stores = diskutil(&["list", "-plist"])
            .map(|l| container_stores(&l))
            .unwrap_or_default();
        vec![DeviceId(format!(
            "/dev/{}",
            physical_disk_for(&from, &stores)
        ))]
    }

    fn is_network_path(path: &Path) -> bool {
        statfs(path)
            .map(|(_, t)| matches!(t.as_str(), "smbfs" | "nfs" | "afpfs" | "webdav" | "ftp"))
            .unwrap_or(false)
    }

    fn free_space(path: &Path) -> Option<u64> {
        let c = CString::new(path.as_os_str().to_string_lossy().as_bytes()).ok()?;
        // SAFETY: zeroed statvfs is valid plain data; `c` is NUL-terminated.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: valid path pointer and writable struct.
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }

    fn total_ram() -> Option<u64> {
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let name = c"hw.memsize";
        // SAFETY: valid NUL-terminated name, output buffer of `len` bytes.
        let r = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut v as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (r == 0).then_some(v)
    }

    fn chown(path: &Path, uid: u32, gid: u32) {
        let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
    }
}

fn statfs(path: &Path) -> Option<(String, String)> {
    let c = CString::new(path.as_os_str().to_string_lossy().as_bytes()).ok()?;
    // SAFETY: zeroed statfs is a valid all-integer struct; `c` is NUL-terminated.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid path pointer and writable struct.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let cs = |a: &[libc::c_char]| {
        let bytes: Vec<u8> = a
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Some((cs(&st.f_mntfromname), cs(&st.f_fstypename)))
}

fn mount_from(path: &Path) -> Option<String> {
    let (from, _) = statfs(path)?;
    from.starts_with("/dev/").then_some(from)
}

struct MacLock {
    disk: Option<String>,
    name: String,
}

impl DiskLock for MacLock {
    fn describe(&self) -> String {
        if self.disk.is_some() {
            format!("{} (unmounted)", self.name)
        } else {
            format!("{} (read-only)", self.name)
        }
    }
}

impl Drop for MacLock {
    fn drop(&mut self) {
        if let Some(d) = &self.disk {
            let _ = Command::new("/usr/sbin/diskutil")
                .args(["mountDisk", d])
                .output();
        }
    }
}
