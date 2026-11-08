//! MBR and GPT partition tables, plus filesystem detection from boot sectors.

use super::read_vec;
use crate::device::{FsType, PartitionInfo};
use crate::io::BlockRead;
use crate::util::{le16, le32, le64, utf16le};

/// Detect partitions. A disk without a table but with a filesystem at sector 0 is returned
/// as one partition covering the whole disk.
pub fn detect(r: &dyn BlockRead) -> Vec<PartitionInfo> {
    let Some(s0) = read_vec(r, 0, 512) else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let has_sig = s0[510] == 0x55 && s0[511] == 0xAA;
    let protective = has_sig && (0..4).any(|i| s0[446 + i * 16 + 4] == 0xEE);
    if protective {
        for ss in [r.sector_size().max(512) as u64, 512, 4096] {
            parts = gpt(r, ss);
            if !parts.is_empty() {
                break;
            }
        }
    } else if has_sig && detect_fs_bytes(&s0, None) == FsType::Unknown {
        parts = mbr(r, &s0);
    }
    if parts.is_empty() {
        let fs = detect_fs(r, 0);
        if fs != FsType::Unknown {
            parts.push(PartitionInfo {
                index: 0,
                offset: 0,
                len: r.size(),
                fs,
                label: None,
                mount_points: Vec::new(),
            });
        }
    }
    for p in &mut parts {
        if p.fs == FsType::Unknown {
            p.fs = detect_fs(r, p.offset);
        }
        if p.label.is_none() {
            p.label = fat_label(r, p);
        }
    }
    parts
}

fn mbr(r: &dyn BlockRead, s0: &[u8]) -> Vec<PartitionInfo> {
    let size = r.size();
    let mut out = Vec::new();
    let mut ext_base = None;
    for i in 0..4 {
        let e = &s0[446 + i * 16..446 + i * 16 + 16];
        if e[0] != 0 && e[0] != 0x80 {
            return Vec::new();
        }
        let ty = e[4];
        let lba = le32(e, 8).unwrap_or(0) as u64;
        let count = le32(e, 12).unwrap_or(0) as u64;
        if ty == 0 || count == 0 {
            continue;
        }
        let (off, len) = (lba * 512, count * 512);
        if off >= size {
            continue;
        }
        if matches!(ty, 0x05 | 0x0F | 0x85) {
            ext_base = Some(lba);
            continue;
        }
        out.push(PartitionInfo {
            index: i as u32 + 1,
            offset: off,
            len: len.min(size - off),
            fs: FsType::Unknown,
            label: None,
            mount_points: Vec::new(),
        });
    }
    if let Some(base) = ext_base {
        let mut ebr = base;
        let mut idx = 5;
        for _ in 0..128 {
            let Some(s) = read_vec(r, ebr * 512, 512) else {
                break;
            };
            if s[510] != 0x55 || s[511] != 0xAA {
                break;
            }
            let e1 = &s[446..462];
            let e2 = &s[462..478];
            let lba = le32(e1, 8).unwrap_or(0) as u64;
            let count = le32(e1, 12).unwrap_or(0) as u64;
            if e1[4] != 0 && count > 0 {
                let off = (ebr + lba) * 512;
                if off < size {
                    out.push(PartitionInfo {
                        index: idx,
                        offset: off,
                        len: (count * 512).min(size - off),
                        fs: FsType::Unknown,
                        label: None,
                        mount_points: Vec::new(),
                    });
                    idx += 1;
                }
            }
            let next = le32(e2, 8).unwrap_or(0) as u64;
            if e2[4] == 0 || next == 0 {
                break;
            }
            ebr = base + next;
        }
    }
    out
}

fn gpt(r: &dyn BlockRead, ss: u64) -> Vec<PartitionInfo> {
    let Some(h) = read_vec(r, ss, 92) else {
        return Vec::new();
    };
    if &h[0..8] != b"EFI PART" {
        return Vec::new();
    }
    let entries_lba = le64(&h, 72).unwrap_or(0);
    let count = le32(&h, 80).unwrap_or(0).min(512) as u64;
    let esize = le32(&h, 84).unwrap_or(128) as u64;
    if !(128..=4096).contains(&esize) || entries_lba == 0 {
        return Vec::new();
    }
    let Some(table) = read_vec(r, entries_lba * ss, (count * esize) as usize) else {
        return Vec::new();
    };
    let size = r.size();
    let mut out = Vec::new();
    for i in 0..count as usize {
        let e = &table[i * esize as usize..(i + 1) * esize as usize];
        if e[0..16].iter().all(|&b| b == 0) {
            continue;
        }
        let first = le64(e, 32).unwrap_or(0);
        let last = le64(e, 40).unwrap_or(0);
        if last < first {
            continue;
        }
        let off = first * ss;
        if off >= size {
            continue;
        }
        let name = utf16le(&e[56..128.min(e.len())]);
        out.push(PartitionInfo {
            index: i as u32 + 1,
            offset: off,
            len: ((last - first + 1) * ss).min(size - off),
            fs: FsType::Unknown,
            label: (!name.is_empty()).then_some(name),
            mount_points: Vec::new(),
        });
    }
    out
}

/// Filesystem at `offset`, from its boot sector / superblock.
pub fn detect_fs(r: &dyn BlockRead, offset: u64) -> FsType {
    let Some(b) = read_vec(r, offset, 512) else {
        return FsType::Unknown;
    };
    let more = read_vec(r, offset + 1024, 1024);
    detect_fs_bytes(&b, more.as_deref())
}

fn detect_fs_bytes(b: &[u8], at1024: Option<&[u8]>) -> FsType {
    if &b[3..11] == b"NTFS    " {
        return FsType::Ntfs;
    }
    if &b[3..11] == b"EXFAT   " {
        return FsType::ExFat;
    }
    if &b[82..87] == b"FAT32" {
        return FsType::Fat32;
    }
    if &b[54..59] == b"FAT16" || &b[54..62] == b"FAT     " {
        return FsType::Fat16;
    }
    if &b[54..59] == b"FAT12" {
        return FsType::Fat12;
    }
    if &b[32..36] == b"NXSB" {
        return FsType::Apfs;
    }
    if let Some(s) = at1024 {
        if le16(s, 56) == Some(0xEF53) {
            return FsType::Ext;
        }
        if &s[0..2] == b"H+" || &s[0..2] == b"HX" {
            return FsType::HfsPlus;
        }
    }
    if &b[0..4] == b"XFSB" {
        return FsType::Xfs;
    }
    FsType::Unknown
}

fn fat_label(r: &dyn BlockRead, p: &PartitionInfo) -> Option<String> {
    let off = match p.fs {
        FsType::Fat32 => 71,
        FsType::Fat16 | FsType::Fat12 => 43,
        _ => return None,
    };
    let b = read_vec(r, p.offset, 512)?;
    let s = String::from_utf8_lossy(&b[off..off + 11])
        .trim()
        .to_string();
    (!s.is_empty() && s != "NO NAME").then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::MemSource;

    #[test]
    fn mbr_partitions() {
        let mut d = vec![0u8; 4 << 20];
        d[510] = 0x55;
        d[511] = 0xAA;
        let e = &mut d[446..462];
        e[4] = 0x07;
        e[8..12].copy_from_slice(&2048u32.to_le_bytes());
        e[12..16].copy_from_slice(&4096u32.to_le_bytes());
        // NTFS boot sector at 1 MiB
        d[(1 << 20) + 3..(1 << 20) + 11].copy_from_slice(b"NTFS    ");
        let p = detect(&MemSource::new(d));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].offset, 1 << 20);
        assert_eq!(p[0].len, 2 << 20);
        assert_eq!(p[0].fs, FsType::Ntfs);
    }
}
