//! Linux device access: `/sys/block`, `/proc/self/mountinfo`, `O_DIRECT` and `BLKROSET`.

use super::{BusType, DeviceId, DeviceInfo, DeviceKind, DiskLock, FsType, PartitionInfo, Platform};
use crate::error::{CoreError, Result};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

const BLKROSET: u64 = 0x125D;
const BLKROGET: u64 = 0x125E;

pub(crate) struct Os;

fn read_trim(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_u64(p: impl AsRef<Path>) -> Option<u64> {
    read_trim(p)?.parse().ok()
}

struct Mount {
    source: String,
    target: String,
    fstype: String,
}

fn unescape(s: &str) -> String {
    s.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\134", "\\")
}

fn mounts() -> Vec<Mount> {
    let Ok(text) = fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let target = left.split(' ').nth(4)?;
            let mut r = right.split(' ');
            let fstype = r.next()?;
            let source = r.next()?;
            Some(Mount {
                source: source.to_string(),
                target: unescape(target),
                fstype: fstype.to_string(),
            })
        })
        .collect()
}

fn labels() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(rd) = fs::read_dir("/dev/disk/by-label") {
        for e in rd.flatten() {
            if let Ok(t) = fs::canonicalize(e.path()) {
                map.insert(
                    t.to_string_lossy().into_owned(),
                    unescape(&e.file_name().to_string_lossy().replace("\\x20", " ")),
                );
            }
        }
    }
    map
}

/// Canonical `/dev/<name>` for a mount source (follows `/dev/disk/by-*` links).
fn canon_dev(src: &str) -> String {
    fs::canonicalize(src)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| src.to_string())
}

/// Whole-disk names under `/sys/class/block/<name>`, following partitions and dm/md slaves.
fn whole_disks(name: &str, depth: u32, out: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    let sys = PathBuf::from("/sys/class/block").join(name);
    if let Ok(rd) = fs::read_dir(sys.join("slaves")) {
        let slaves: Vec<_> = rd.flatten().collect();
        if !slaves.is_empty() {
            for s in slaves {
                whole_disks(&s.file_name().to_string_lossy(), depth + 1, out);
            }
            return;
        }
    }
    if sys.join("partition").exists() {
        if let Ok(real) = fs::canonicalize(&sys) {
            if let Some(parent) = real.parent().and_then(|p| p.file_name()) {
                out.push(parent.to_string_lossy().into_owned());
                return;
            }
        }
    }
    out.push(name.to_string());
}

fn root_disks(ms: &[Mount]) -> Vec<String> {
    let mut out = Vec::new();
    for m in ms
        .iter()
        .filter(|m| m.target == "/" || m.target == "/boot" || m.target == "/usr")
    {
        let dev = canon_dev(&m.source);
        if let Some(name) = dev.strip_prefix("/dev/") {
            whole_disks(name, 0, &mut out);
        }
    }
    out
}

fn ioctl_int(f: &File, req: u64, val: &mut libc::c_int) -> io::Result<()> {
    // SAFETY: `f` is an open fd; BLKROSET/BLKROGET take a pointer to an int which is valid.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), req as _, val as *mut libc::c_int) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl Platform for Os {
    fn list_devices() -> Vec<DeviceInfo> {
        let ms = mounts();
        let lbl = labels();
        let sys_disks = root_disks(&ms);
        let mut mount_map: HashMap<String, Vec<String>> = HashMap::new();
        for m in &ms {
            if m.source.starts_with('/') {
                mount_map
                    .entry(canon_dev(&m.source))
                    .or_default()
                    .push(m.target.clone());
            }
        }
        let fs_map: HashMap<String, String> = ms
            .iter()
            .map(|m| (canon_dev(&m.source), m.fstype.clone()))
            .collect();
        let Ok(rd) = fs::read_dir("/sys/block") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if ["loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd"]
                .iter()
                .any(|p| name.starts_with(p))
            {
                continue;
            }
            let base = PathBuf::from("/sys/block").join(&name);
            let size = read_u64(base.join("size")).unwrap_or(0) * 512;
            if size == 0 {
                continue;
            }
            let sector = read_u64(base.join("queue/logical_block_size")).unwrap_or(512) as u32;
            let phys =
                read_u64(base.join("queue/physical_block_size")).unwrap_or(sector as u64) as u32;
            let rotational = read_trim(base.join("queue/rotational")).map(|s| s == "1");
            let removable = read_trim(base.join("removable")).as_deref() == Some("1");
            let vendor = read_trim(base.join("device/vendor"));
            let model = read_trim(base.join("device/model"))
                .or_else(|| read_trim(base.join("device/name")))
                .unwrap_or_else(|| name.clone());
            let model = match vendor {
                Some(v) if !model.starts_with(&v) && v != "ATA" => format!("{v} {model}"),
                _ => model,
            };
            let serial = read_trim(base.join("device/serial"))
                .or_else(|| read_trim(base.join("device/wwid")));
            let link = fs::read_link(&base)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bus = if name.starts_with("nvme") {
                BusType::Nvme
            } else if name.starts_with("mmcblk") {
                BusType::Sd
            } else if link.contains("/usb") {
                BusType::Usb
            } else if name.starts_with("vd") || link.contains("virtio") {
                BusType::Virtual
            } else if name.starts_with("sd") || name.starts_with("hd") {
                BusType::Sata
            } else {
                BusType::Unknown
            };
            let mut partitions = Vec::new();
            if let Ok(rd) = fs::read_dir(&base) {
                for e in rd.flatten() {
                    let pname = e.file_name().to_string_lossy().into_owned();
                    let pp = e.path();
                    if !pname.starts_with(&name) || !pp.join("partition").exists() {
                        continue;
                    }
                    let dev = format!("/dev/{pname}");
                    partitions.push(PartitionInfo {
                        index: read_u64(pp.join("partition")).unwrap_or(0) as u32,
                        offset: read_u64(pp.join("start")).unwrap_or(0) * 512,
                        len: read_u64(pp.join("size")).unwrap_or(0) * 512,
                        fs: fs_map
                            .get(&dev)
                            .map(|s| FsType::from_name(s))
                            .unwrap_or(FsType::Unknown),
                        label: lbl.get(&dev).cloned(),
                        mount_points: mount_map.get(&dev).cloned().unwrap_or_default(),
                    });
                }
            }
            partitions.sort_by_key(|p| p.offset);
            let path = PathBuf::from(format!("/dev/{name}"));
            let readable = File::open(&path).is_ok();
            out.push(DeviceInfo {
                id: DeviceId(path.to_string_lossy().into_owned()),
                path,
                kind: DeviceKind::PhysicalDisk,
                model,
                serial,
                bus,
                size,
                sector_size: sector.max(512),
                physical_sector_size: phys.max(512),
                rotational,
                removable: removable || bus == BusType::Usb,
                is_system: sys_disks.contains(&name),
                partitions,
                readable,
            });
        }
        out
    }

    fn open_raw(path: &Path) -> io::Result<File> {
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECT | libc::O_CLOEXEC)
            .open(path)
        {
            Ok(f) => Ok(f),
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) => OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_CLOEXEC)
                .open(path),
            Err(e) => Err(e),
        }
    }

    fn device_size(f: &File) -> io::Result<u64> {
        let mut f = f.try_clone()?;
        let n = f.seek(SeekFrom::End(0))?;
        f.seek(SeekFrom::Start(0))?;
        Ok(n)
    }

    fn pread(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        f.read_at(buf, offset)
    }

    fn lock(dev: &DeviceInfo, unmount: bool) -> Result<Box<dyn DiskLock>> {
        let mut lock = LinuxLock {
            devs: Vec::new(),
            name: dev.id.0.clone(),
        };
        let mut failed = Vec::new();
        if unmount {
            for p in &dev.partitions {
                for mp in &p.mount_points {
                    let Ok(c) = CString::new(mp.as_str()) else {
                        continue;
                    };
                    // SAFETY: `c` is a valid NUL-terminated path.
                    if unsafe { libc::umount2(c.as_ptr(), 0) } != 0 {
                        failed.push(mp.clone());
                    }
                }
            }
        }
        if !failed.is_empty() {
            return Err(CoreError::LockFailed(failed));
        }
        let mut targets = vec![dev.path.clone()];
        if let Some(name) = dev
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        {
            if let Ok(rd) = fs::read_dir(format!("/sys/block/{name}")) {
                for e in rd.flatten() {
                    let pname = e.file_name().to_string_lossy().into_owned();
                    if pname.starts_with(&name) && e.path().join("partition").exists() {
                        targets.push(PathBuf::from(format!("/dev/{pname}")));
                    }
                }
            }
        }
        for t in targets {
            let f = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_CLOEXEC)
                .open(&t)
            {
                Ok(f) => f,
                Err(_) => {
                    failed.push(t.display().to_string());
                    continue;
                }
            };
            let mut prev: libc::c_int = 0;
            let _ = ioctl_int(&f, BLKROGET, &mut prev);
            let mut one: libc::c_int = 1;
            if ioctl_int(&f, BLKROSET, &mut one).is_err() {
                failed.push(t.display().to_string());
                continue;
            }
            lock.devs.push((f, prev));
        }
        if !failed.is_empty() {
            return Err(CoreError::LockFailed(failed));
        }
        Ok(Box::new(lock))
    }

    fn disks_of_path(path: &Path) -> Vec<DeviceId> {
        let mut names = Vec::new();
        if let Ok(meta) = fs::metadata(path) {
            let dev = meta.dev();
            let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
            let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
            if major != 0 {
                if let Ok(real) = fs::canonicalize(format!("/sys/dev/block/{major}:{minor}")) {
                    if let Some(n) = real.file_name() {
                        whole_disks(&n.to_string_lossy(), 0, &mut names);
                    }
                }
            }
        }
        if names.is_empty() {
            // Anonymous device numbers (btrfs, overlay): use the longest matching mount.
            let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let ms = mounts();
            if let Some(m) = ms
                .iter()
                .filter(|m| canon.starts_with(&m.target))
                .max_by_key(|m| m.target.len())
            {
                let dev = canon_dev(&m.source);
                if let Some(n) = dev.strip_prefix("/dev/") {
                    whole_disks(n, 0, &mut names);
                }
            }
        }
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|n| DeviceId(format!("/dev/{n}")))
            .collect()
    }

    fn is_network_path(path: &Path) -> bool {
        let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        mounts()
            .iter()
            .filter(|m| canon.starts_with(&m.target))
            .max_by_key(|m| m.target.len())
            .map(|m| {
                matches!(
                    m.fstype.as_str(),
                    "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "fuse.sshfs" | "9p" | "afs"
                )
            })
            .unwrap_or(false)
    }

    fn free_space(path: &Path) -> Option<u64> {
        unix_free_space(path)
    }

    fn total_ram() -> Option<u64> {
        let text = fs::read_to_string("/proc/meminfo").ok()?;
        let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }

    fn chown(path: &Path, uid: u32, gid: u32) {
        let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
    }
}

pub(super) fn unix_free_space(path: &Path) -> Option<u64> {
    let c = CString::new(path.as_os_str().to_string_lossy().as_bytes()).ok()?;
    // SAFETY: zeroed statvfs is valid plain data; `c` is NUL-terminated.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid path pointer and writable struct.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

struct LinuxLock {
    devs: Vec<(File, libc::c_int)>,
    name: String,
}

impl DiskLock for LinuxLock {
    fn describe(&self) -> String {
        format!("{} (read-only)", self.name)
    }
}

impl Drop for LinuxLock {
    fn drop(&mut self) {
        for (f, prev) in &mut self.devs {
            let _ = ioctl_int(f, BLKROSET, prev);
        }
    }
}
