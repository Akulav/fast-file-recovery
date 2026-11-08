//! exFAT quick scan: directory entry sets with `InUse = 0`.

use super::{clean_name, read_vec, QuickFile, QuickScanner, ScanControl};
use crate::carve::Confidence;
use crate::device::PartitionInfo;
use crate::error::{CoreError, Result};
use crate::io::BlockRead;
use crate::util::{dos_to_unix, le16, le32, le64};
use std::collections::HashSet;

pub struct ExFat;

struct Geo {
    part_off: u64,
    cluster_bytes: u64,
    heap_off: u64,
    clusters: u32,
    root: u32,
    fat: Vec<u8>,
}

impl Geo {
    fn parse(r: &dyn BlockRead, part: &PartitionInfo) -> Option<Geo> {
        let b = read_vec(r, part.offset, 512)?;
        if &b[3..11] != b"EXFAT   " {
            return None;
        }
        let bps_shift = b[108] as u32;
        let spc_shift = b[109] as u32;
        if !(9..=12).contains(&bps_shift) || spc_shift > 25 - bps_shift {
            return None;
        }
        let bps = 1u64 << bps_shift;
        let fat_off = le32(&b, 80)? as u64 * bps;
        let fat_len = le32(&b, 84)? as u64 * bps;
        let heap = le32(&b, 88)? as u64 * bps;
        let clusters = le32(&b, 92)?;
        let root = le32(&b, 96)?;
        let fat = read_vec(r, part.offset + fat_off, fat_len.min(512 << 20) as usize)?;
        Some(Geo {
            part_off: part.offset,
            cluster_bytes: bps << spc_shift,
            heap_off: heap,
            clusters,
            root,
            fat,
        })
    }

    fn valid(&self, c: u32) -> bool {
        c >= 2 && c < self.clusters + 2
    }

    fn off(&self, c: u32) -> u64 {
        self.part_off + self.heap_off + (c as u64 - 2) * self.cluster_bytes
    }

    fn chain(&self, start: u32, max: usize) -> Vec<u32> {
        let mut v = Vec::new();
        let mut c = start;
        while self.valid(c) && v.len() < max {
            v.push(c);
            let n = le32(&self.fat, c as usize * 4).unwrap_or(0);
            if n >= 0xFFFF_FFF7 || n == c || n == 0 {
                break;
            }
            c = n;
        }
        v
    }

    /// Extents for a file of `len` bytes starting at `first`.
    fn extents(&self, first: u32, len: u64, no_chain: bool) -> Vec<(u64, u64)> {
        let need = len.div_ceil(self.cluster_bytes) as usize;
        if !no_chain {
            let ch = self.chain(first, need + 1);
            if ch.len() >= need && need > 0 {
                let mut ext: Vec<(u64, u64)> = Vec::new();
                let mut left = len;
                for c in ch.into_iter().take(need) {
                    let n = left.min(self.cluster_bytes);
                    let o = self.off(c);
                    match ext.last_mut() {
                        Some(l) if l.0 + l.1 == o => l.1 += n,
                        _ => ext.push((o, n)),
                    }
                    left -= n;
                }
                return ext;
            }
        }
        vec![(self.off(first), len)]
    }
}

fn ts(v: u32) -> Option<i64> {
    dos_to_unix((v >> 16) as u16, (v & 0xFFFF) as u16)
}

impl QuickScanner for ExFat {
    fn scan(
        &self,
        r: &dyn BlockRead,
        part: &PartitionInfo,
        ctl: &dyn ScanControl,
        out: &mut dyn FnMut(QuickFile),
    ) -> Result<()> {
        let g = Geo::parse(r, part)
            .ok_or_else(|| CoreError::Invalid("The exFAT boot sector is damaged.".into()))?;
        // (first cluster, byte length or 0 for chain, contiguous?, path, inside deleted dir)
        let mut queue: Vec<(u32, u64, bool, String, bool, u32)> =
            vec![(g.root, 0, false, String::new(), false, 0)];
        let mut visited = HashSet::new();
        let mut done = 0usize;
        while let Some((first, len, contiguous, path, parent_deleted, depth)) = queue.pop() {
            if ctl.should_stop() {
                return Err(CoreError::Cancelled);
            }
            done += 1;
            ctl.progress(done as f32 / (done + queue.len()) as f32);
            if !g.valid(first) || !visited.insert(first) {
                continue;
            }
            let data: Vec<u8> = if contiguous {
                let l = len.clamp(32, 4 << 20) as usize;
                read_vec(r, g.off(first), l).unwrap_or_default()
            } else {
                let mut v = Vec::new();
                for c in g.chain(first, 8192) {
                    if let Some(d) = read_vec(r, g.off(c), g.cluster_bytes as usize) {
                        v.extend(d);
                    }
                }
                v
            };
            let mut i = 0usize;
            while i + 32 <= data.len() {
                let ty = data[i];
                if ty == 0 {
                    break;
                }
                if ty != 0x85 && ty != 0x05 {
                    i += 32;
                    continue;
                }
                let deleted = ty == 0x05 || parent_deleted;
                let e = &data[i..i + 32];
                let secondary = e[1] as usize;
                let attrs = le16(e, 4).unwrap_or(0);
                let created = ts(le32(e, 8).unwrap_or(0));
                let modified = ts(le32(e, 12).unwrap_or(0));
                if !(2..=18).contains(&secondary) || i + 32 * (secondary + 1) > data.len() {
                    i += 32;
                    continue;
                }
                let s = &data[i + 32..i + 64];
                if s[0] & 0x7F != 0x40 {
                    i += 32;
                    continue;
                }
                let flags = s[1];
                let name_len = s[3] as usize;
                let first_c = le32(s, 20).unwrap_or(0);
                let size = le64(s, 24).unwrap_or(0);
                let mut units: Vec<u16> = Vec::new();
                for k in 2..=secondary {
                    let ne = &data[i + k * 32..i + k * 32 + 32];
                    if ne[0] & 0x7F != 0x41 {
                        break;
                    }
                    for c in ne[2..32].as_chunks::<2>().0 {
                        units.push(u16::from_le_bytes([c[0], c[1]]));
                    }
                }
                units.truncate(name_len);
                let name = clean_name(&String::from_utf16_lossy(&units));
                let no_chain = flags & 2 != 0;
                if attrs & 0x10 != 0 {
                    if depth < 64 && g.valid(first_c) {
                        let child = if path.is_empty() {
                            name
                        } else {
                            format!("{path}/{name}")
                        };
                        queue.push((
                            first_c,
                            size,
                            no_chain || deleted,
                            child,
                            deleted,
                            depth + 1,
                        ));
                    }
                } else if deleted && size > 0 && g.valid(first_c) {
                    let extents = g.extents(first_c, size, no_chain);
                    out(QuickFile {
                        name,
                        path: path.clone(),
                        size,
                        extents,
                        inline: None,
                        modified,
                        created,
                        confidence: Confidence::Medium,
                    });
                }
                i += 32 * (secondary + 1);
            }
        }
        Ok(())
    }
}
