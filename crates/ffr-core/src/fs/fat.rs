//! FAT12/16/32 quick scan: deleted (`0xE5`) directory entries with long file names.
//! The FAT chain of a deleted file is cleared, so data is assumed to be contiguous.

use super::{clean_name, read_vec, QuickFile, QuickScanner, ScanControl};
use crate::carve::Confidence;
use crate::device::PartitionInfo;
use crate::error::{CoreError, Result};
use crate::io::BlockRead;
use crate::util::{dos_to_unix, le16, le32};
use std::collections::HashSet;

pub struct Fat;

struct Geo {
    part_off: u64,
    cluster_bytes: u64,
    data_off: u64,
    root_off: u64,
    root_bytes: u64,
    root_clus: u32,
    clusters: u32,
    bits: u8,
    fat: Vec<u8>,
}

impl Geo {
    fn parse(r: &dyn BlockRead, part: &PartitionInfo) -> Option<Geo> {
        let b = read_vec(r, part.offset, 512)?;
        let bps = le16(&b, 11)? as u64;
        let spc = b[13] as u64;
        let reserved = le16(&b, 14)? as u64;
        let nfats = b[16] as u64;
        let root_entries = le16(&b, 17)? as u64;
        let total = match le16(&b, 19)? {
            0 => le32(&b, 32)? as u64,
            n => n as u64,
        };
        let fatsz = match le16(&b, 22)? {
            0 => le32(&b, 36)? as u64,
            n => n as u64,
        };
        if !matches!(bps, 512 | 1024 | 2048 | 4096) || spc == 0 || !spc.is_power_of_two() {
            return None;
        }
        if nfats == 0 || fatsz == 0 || reserved == 0 {
            return None;
        }
        let root_sectors = (root_entries * 32).div_ceil(bps);
        let first_data = reserved + nfats * fatsz + root_sectors;
        if total <= first_data {
            return None;
        }
        let clusters = ((total - first_data) / spc) as u32;
        let bits = if clusters < 4085 {
            12
        } else if clusters < 65_525 {
            16
        } else {
            32
        };
        let fat_bytes = (fatsz * bps).min(512 << 20) as usize;
        let fat = read_vec(r, part.offset + reserved * bps, fat_bytes)?;
        Some(Geo {
            part_off: part.offset,
            cluster_bytes: spc * bps,
            data_off: first_data * bps,
            root_off: (reserved + nfats * fatsz) * bps,
            root_bytes: root_sectors * bps,
            root_clus: if bits == 32 { le32(&b, 44)? } else { 0 },
            clusters,
            bits,
            fat,
        })
    }

    fn valid(&self, cl: u32) -> bool {
        cl >= 2 && cl < self.clusters + 2
    }

    fn entry(&self, cl: u32) -> u32 {
        let c = cl as usize;
        match self.bits {
            12 => {
                let o = c + c / 2;
                let v = le16(&self.fat, o).unwrap_or(0) as u32;
                if c & 1 == 1 {
                    v >> 4
                } else {
                    v & 0xFFF
                }
            }
            16 => le16(&self.fat, c * 2).unwrap_or(0) as u32,
            _ => le32(&self.fat, c * 4).unwrap_or(0) & 0x0FFF_FFFF,
        }
    }

    fn is_end(&self, v: u32) -> bool {
        match self.bits {
            12 => v >= 0xFF8,
            16 => v >= 0xFFF8,
            _ => v >= 0x0FFF_FFF8,
        }
    }

    fn cluster_off(&self, cl: u32) -> u64 {
        self.part_off + self.data_off + (cl as u64 - 2) * self.cluster_bytes
    }

    fn chain(&self, start: u32, max: usize) -> Vec<u32> {
        let mut v = Vec::new();
        let mut c = start;
        while self.valid(c) && v.len() < max {
            v.push(c);
            let n = self.entry(c);
            if self.is_end(n) || n == c {
                break;
            }
            c = n;
        }
        v
    }
}

#[derive(Clone, Copy)]
enum DirLoc {
    Root16,
    Chain(u32),
    Contiguous(u32),
}

struct Entry {
    name: String,
    is_dir: bool,
    deleted: bool,
    cluster: u32,
    size: u64,
    mtime: Option<i64>,
    ctime: Option<i64>,
}

fn lfn_checksum(short: &[u8; 11]) -> u8 {
    short.iter().fold(0u8, |s, &c| {
        ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c)
    })
}

fn short_name(n: &[u8], case: u8, deleted: bool) -> String {
    let mut base: Vec<u8> = n[0..8].to_vec();
    if deleted {
        base[0] = b'_';
    } else if base[0] == 0x05 {
        base[0] = 0xE5;
    }
    let mut b = String::from_utf8_lossy(&base).trim_end().to_string();
    let mut e = String::from_utf8_lossy(&n[8..11]).trim_end().to_string();
    if case & 0x08 != 0 {
        b = b.to_lowercase();
    }
    if case & 0x10 != 0 {
        e = e.to_lowercase();
    }
    if e.is_empty() {
        b
    } else {
        format!("{b}.{e}")
    }
}

fn parse_dir(data: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut lfn: Vec<([u16; 13], u8)> = Vec::new();
    for e in data.as_chunks::<32>().0 {
        if e[0] == 0 {
            break;
        }
        let attr = e[11];
        if attr == 0x0F {
            let mut u = [0u16; 13];
            let idx = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (k, &o) in idx.iter().enumerate() {
                u[k] = u16::from_le_bytes([e[o], e[o + 1]]);
            }
            lfn.push((u, e[13]));
            continue;
        }
        let parts = std::mem::take(&mut lfn);
        if attr & 0x08 != 0 || e[0] == b'.' {
            continue;
        }
        let deleted = e[0] == 0xE5;
        let mut short = [0u8; 11];
        short.copy_from_slice(&e[0..11]);
        let long = if !parts.is_empty() && parts.iter().all(|p| p.1 == parts[0].1) {
            let sum = parts[0].1;
            let ok = if deleted {
                (0x20u8..0x7F).any(|c| {
                    short[0] = c;
                    lfn_checksum(&short) == sum
                })
            } else {
                lfn_checksum(&short) == sum
            };
            ok.then(|| {
                let units: Vec<u16> = parts
                    .iter()
                    .rev()
                    .flat_map(|p| p.0)
                    .take_while(|&c| c != 0 && c != 0xFFFF)
                    .collect();
                String::from_utf16_lossy(&units)
            })
        } else {
            None
        };
        let name = long
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| short_name(&e[0..11], e[12], deleted));
        let hi = le16(e, 20).unwrap_or(0) as u32;
        let lo = le16(e, 26).unwrap_or(0) as u32;
        out.push(Entry {
            name,
            is_dir: attr & 0x10 != 0,
            deleted,
            cluster: (hi << 16) | lo,
            size: le32(e, 28).unwrap_or(0) as u64,
            mtime: dos_to_unix(le16(e, 24).unwrap_or(0), le16(e, 22).unwrap_or(0)),
            ctime: dos_to_unix(le16(e, 16).unwrap_or(0), le16(e, 14).unwrap_or(0)),
        });
    }
    out
}

impl QuickScanner for Fat {
    fn scan(
        &self,
        r: &dyn BlockRead,
        part: &PartitionInfo,
        ctl: &dyn ScanControl,
        out: &mut dyn FnMut(QuickFile),
    ) -> Result<()> {
        let g = Geo::parse(r, part)
            .ok_or_else(|| CoreError::Invalid("The FAT boot sector is damaged.".into()))?;
        let root = if g.bits == 32 {
            DirLoc::Chain(g.root_clus)
        } else {
            DirLoc::Root16
        };
        let mut queue: Vec<(DirLoc, String, bool, u32)> = vec![(root, String::new(), false, 0)];
        let mut visited: HashSet<u32> = HashSet::new();
        let mut done = 0usize;
        while let Some((loc, path, parent_deleted, depth)) = queue.pop() {
            if ctl.should_stop() {
                return Err(CoreError::Cancelled);
            }
            done += 1;
            ctl.progress(done as f32 / (done + queue.len()) as f32);
            let data = match loc {
                DirLoc::Root16 => read_vec(r, part.offset + g.root_off, g.root_bytes as usize),
                DirLoc::Chain(c) | DirLoc::Contiguous(c) if !visited.insert(c) => continue,
                DirLoc::Chain(c) => {
                    let mut v = Vec::new();
                    for cl in g.chain(c, 4096) {
                        if let Some(d) = read_vec(r, g.cluster_off(cl), g.cluster_bytes as usize) {
                            v.extend(d);
                        }
                    }
                    Some(v)
                }
                DirLoc::Contiguous(c) => {
                    let n = (64u64).min((g.clusters + 2 - c) as u64);
                    read_vec(r, g.cluster_off(c), (n * g.cluster_bytes) as usize)
                }
            };
            let Some(data) = data else { continue };
            for e in parse_dir(&data) {
                let deleted = e.deleted || parent_deleted;
                let name = clean_name(&e.name);
                if e.is_dir {
                    if g.valid(e.cluster) && depth < 64 {
                        let child = if path.is_empty() {
                            name
                        } else {
                            format!("{path}/{name}")
                        };
                        let loc = if deleted {
                            DirLoc::Contiguous(e.cluster)
                        } else {
                            DirLoc::Chain(e.cluster)
                        };
                        queue.push((loc, child, deleted, depth + 1));
                    }
                    continue;
                }
                if !deleted || e.size == 0 || !g.valid(e.cluster) {
                    continue;
                }
                let off = g.cluster_off(e.cluster);
                if off + e.size > part.offset + part.len {
                    continue;
                }
                let confidence = if g.entry(e.cluster) == 0 {
                    Confidence::Medium
                } else {
                    Confidence::Low
                };
                out(QuickFile {
                    name,
                    path: path.clone(),
                    size: e.size,
                    extents: vec![(off, e.size)],
                    inline: None,
                    modified: e.mtime,
                    created: e.ctime,
                    confidence,
                });
            }
        }
        Ok(())
    }
}
