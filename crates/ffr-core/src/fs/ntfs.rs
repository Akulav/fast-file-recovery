//! NTFS quick scan: `$MFT` records with the in-use flag clear. Names and parent folders come
//! from `$FILE_NAME`, data from `$DATA` runs (exact extents, also for fragmented files) or
//! resident bytes. `$Bitmap` tells whether a deleted file's clusters were reused.

use super::{clean_name, read_vec, QuickFile, QuickScanner, ScanControl};
use crate::carve::Confidence;
use crate::device::PartitionInfo;
use crate::error::{CoreError, Result};
use crate::io::BlockRead;
use crate::util::{filetime_to_unix, le16, le32, le64, utf16le};
use std::collections::HashMap;

pub struct Ntfs;

const ROOT: u64 = 5;

type Runs = Vec<(Option<u64>, u64)>;

fn apply_fixups(rec: &mut [u8]) -> bool {
    let (Some(off), Some(cnt)) = (le16(rec, 4), le16(rec, 6)) else {
        return false;
    };
    let (off, cnt) = (off as usize, cnt as usize);
    if cnt < 2 || off + cnt * 2 > rec.len() {
        return false;
    }
    let stride = rec.len() / (cnt - 1);
    if stride < 256 {
        return false;
    }
    let usn = [rec[off], rec[off + 1]];
    for i in 1..cnt {
        let end = i * stride - 2;
        if end + 2 > rec.len() || rec[end..end + 2] != usn {
            return false;
        }
        rec[end] = rec[off + 2 * i];
        rec[end + 1] = rec[off + 2 * i + 1];
    }
    true
}

pub(crate) fn decode_runs(data: &[u8]) -> Option<Runs> {
    let mut p = 0usize;
    let mut lcn: i64 = 0;
    let mut out = Vec::new();
    while p < data.len() {
        let h = data[p];
        if h == 0 {
            break;
        }
        let ls = (h & 0xF) as usize;
        let os = (h >> 4) as usize;
        p += 1;
        if ls == 0 || ls > 8 || os > 8 || p + ls + os > data.len() {
            return None;
        }
        let mut len = 0u64;
        for i in 0..ls {
            len |= (data[p + i] as u64) << (8 * i);
        }
        p += ls;
        if os == 0 {
            out.push((None, len));
        } else {
            let mut off: i64 = 0;
            for i in 0..os {
                off |= (data[p + i] as i64) << (8 * i);
            }
            if data[p + os - 1] & 0x80 != 0 && os < 8 {
                off -= 1i64 << (8 * os);
            }
            lcn = lcn.checked_add(off)?;
            if lcn < 0 {
                return None;
            }
            out.push((Some(lcn as u64), len));
        }
        p += os;
        if out.len() > 1_000_000 {
            return None;
        }
    }
    Some(out)
}

enum Data {
    Resident(Vec<u8>, usize),
    NonResident {
        runs: Runs,
        start_vcn: u64,
        size: u64,
        flags: u16,
    },
}

struct Rec {
    in_use: bool,
    is_dir: bool,
    base: u64,
    seq: u16,
    name: Option<(u64, u16, String)>,
    data: Option<Data>,
    created: Option<i64>,
    modified: Option<i64>,
}

fn parse_record(rec: &[u8]) -> Option<Rec> {
    if &rec[0..4] != b"FILE" {
        return None;
    }
    let flags = le16(rec, 22)?;
    let used = (le32(rec, 24)? as usize).min(rec.len());
    let mut r = Rec {
        in_use: flags & 1 != 0,
        is_dir: flags & 2 != 0,
        base: le64(rec, 32)? & 0xFFFF_FFFF_FFFF,
        seq: le16(rec, 16)?,
        name: None,
        data: None,
        created: None,
        modified: None,
    };
    let mut best_ns = u8::MAX;
    let mut p = le16(rec, 20)? as usize;
    for _ in 0..256 {
        let Some(ty) = le32(rec, p) else { break };
        if ty == 0xFFFF_FFFF {
            break;
        }
        let len = le32(rec, p + 4)? as usize;
        if len < 16 || p + len > used {
            break;
        }
        let a = &rec[p..p + len];
        let nonres = a[8] != 0;
        let name_len = a[9];
        let aflags = le16(a, 12)?;
        match (ty, nonres) {
            (0x10, false) => {
                let vo = le16(a, 20)? as usize;
                r.created = le64(a, vo).and_then(filetime_to_unix);
                r.modified = le64(a, vo + 8).and_then(filetime_to_unix);
            }
            (0x30, false) => {
                let vo = le16(a, 20)? as usize;
                let v = a.get(vo..)?;
                let parent = le64(v, 0)?;
                let nlen = *v.get(64)? as usize;
                let ns = *v.get(65)?;
                // Prefer Win32/POSIX names over DOS 8.3 ones (namespace 2).
                let rank = if ns == 2 { 1 } else { 0 };
                if rank < best_ns {
                    best_ns = rank;
                    let name = utf16le(v.get(66..66 + nlen * 2)?);
                    r.name = Some((parent & 0xFFFF_FFFF_FFFF, (parent >> 48) as u16, name));
                }
            }
            (0x80, _) if name_len == 0 => {
                if nonres {
                    let start_vcn = le64(a, 16)?;
                    let ro = le16(a, 32)? as usize;
                    let size = le64(a, 48)?;
                    if let Some(runs) = a.get(ro..).and_then(decode_runs) {
                        r.data = Some(Data::NonResident {
                            runs,
                            start_vcn,
                            size,
                            flags: aflags,
                        });
                    }
                } else {
                    let vl = le32(a, 16)? as usize;
                    let vo = le16(a, 20)? as usize;
                    let v = a.get(vo..vo + vl)?;
                    r.data = Some(Data::Resident(v.to_vec(), p + vo));
                }
            }
            _ => {}
        }
        p += len;
    }
    Some(r)
}

struct DirNode {
    parent: u64,
    parent_seq: u16,
    seq: u16,
    name: String,
}

struct Deleted {
    rec: u64,
    rec_disk: u64,
    parent: u64,
    parent_seq: u16,
    name: String,
    data: Data,
    created: Option<i64>,
    modified: Option<i64>,
}

fn bit_set(bitmap: &[u8], c: u64) -> bool {
    bitmap
        .get((c / 8) as usize)
        .is_some_and(|b| b & (1 << (c % 8)) != 0)
}

impl QuickScanner for Ntfs {
    fn scan(
        &self,
        r: &dyn BlockRead,
        part: &PartitionInfo,
        ctl: &dyn ScanControl,
        out: &mut dyn FnMut(QuickFile),
    ) -> Result<()> {
        let bad = || CoreError::Invalid("The NTFS boot sector is damaged.".into());
        let b = read_vec(r, part.offset, 512).ok_or_else(bad)?;
        let bps = le16(&b, 11).ok_or_else(bad)? as u64;
        let spc_raw = b[13];
        let spc = if spc_raw > 0x80 {
            1u64 << (256 - spc_raw as u32)
        } else {
            spc_raw as u64
        };
        let cluster = bps * spc;
        let mft_lcn = le64(&b, 48).ok_or_else(bad)?;
        let cpr = b[64] as i8;
        let rec_size = if cpr > 0 {
            cpr as u64 * cluster
        } else {
            1u64 << (-(cpr as i32)) as u32
        };
        if !matches!(bps, 512 | 1024 | 2048 | 4096)
            || cluster == 0
            || cluster > 2 << 20
            || !(512..=65_536).contains(&rec_size)
        {
            return Err(bad());
        }
        let mft_off = part.offset + mft_lcn * cluster;
        let read_rec = |off: u64| -> Option<Vec<u8>> {
            let mut v = read_vec(r, off, rec_size as usize)?;
            apply_fixups(&mut v).then_some(v)
        };
        let rec0 = read_rec(mft_off)
            .and_then(|v| parse_record(&v))
            .ok_or_else(bad)?;
        let Some(Data::NonResident {
            runs,
            size: mft_size,
            ..
        }) = rec0.data
        else {
            return Err(bad());
        };
        let to_extents = |runs: &Runs| -> Vec<(u64, u64)> {
            runs.iter()
                .map(|(lcn, len)| match lcn {
                    Some(l) => (part.offset + l * cluster, len * cluster),
                    None => (u64::MAX, len * cluster),
                })
                .collect()
        };
        let mft_ext = to_extents(&runs);
        // $Bitmap (record 6) for reuse checks.
        let bitmap: Vec<u8> = read_rec(mft_off + 6 * rec_size)
            .and_then(|v| parse_record(&v))
            .and_then(|rec| match rec.data {
                Some(Data::NonResident { runs, size, .. }) if size < 512 << 20 => {
                    let mut v = Vec::with_capacity(size as usize);
                    for (off, len) in to_extents(&runs) {
                        if off == u64::MAX {
                            v.resize(v.len() + len as usize, 0);
                        } else {
                            v.extend(read_vec(r, off, len as usize)?);
                        }
                    }
                    v.truncate(size as usize);
                    Some(v)
                }
                _ => None,
            })
            .unwrap_or_default();

        let total_recs = mft_size / rec_size;
        let mut dirs: HashMap<u64, DirNode> = HashMap::new();
        let mut deleted: Vec<Deleted> = Vec::new();
        let mut ext_data: HashMap<u64, Vec<(u64, Runs)>> = HashMap::new();
        let mut recno: u64 = 0;
        const BATCH: u64 = 4 << 20;
        'outer: for (eoff, elen) in mft_ext {
            if eoff == u64::MAX {
                recno += elen / rec_size;
                continue;
            }
            let mut done = 0u64;
            while done < elen {
                if recno >= total_recs {
                    break 'outer;
                }
                if ctl.should_stop() {
                    return Err(CoreError::Cancelled);
                }
                ctl.progress(recno as f32 / total_recs.max(1) as f32);
                let n = (elen - done).min(BATCH);
                let Some(batch) = read_vec(r, eoff + done, n as usize) else {
                    recno += n / rec_size;
                    done += n;
                    continue;
                };
                for (k, chunk) in batch.chunks_exact(rec_size as usize).enumerate() {
                    let this = recno;
                    recno += 1;
                    if this >= total_recs {
                        break;
                    }
                    let mut rec = chunk.to_vec();
                    if !apply_fixups(&mut rec) {
                        continue;
                    }
                    let Some(p) = parse_record(&rec) else {
                        continue;
                    };
                    if p.base != 0 {
                        if let Some(Data::NonResident {
                            runs, start_vcn, ..
                        }) = p.data
                        {
                            ext_data.entry(p.base).or_default().push((start_vcn, runs));
                        }
                        continue;
                    }
                    let Some((parent, parent_seq, name)) = p.name else {
                        continue;
                    };
                    if p.is_dir {
                        dirs.insert(
                            this,
                            DirNode {
                                parent,
                                parent_seq,
                                seq: p.seq,
                                name,
                            },
                        );
                    } else if !p.in_use && this >= 24 {
                        if let Some(data) = p.data {
                            deleted.push(Deleted {
                                rec: this,
                                rec_disk: eoff + done + k as u64 * rec_size,
                                parent,
                                parent_seq,
                                name,
                                data,
                                created: p.created,
                                modified: p.modified,
                            });
                        }
                    }
                }
                done += n;
            }
        }

        let mut path_cache: HashMap<u64, String> = HashMap::new();
        let mut path_of = |mut rec: u64, mut seq: u16| -> String {
            let start = (rec, seq);
            if let Some(p) = path_cache.get(&rec) {
                return p.clone();
            }
            let mut parts: Vec<String> = Vec::new();
            for _ in 0..64 {
                if rec == ROOT {
                    break;
                }
                match dirs.get(&rec) {
                    Some(d) if d.seq == seq || seq == 0 => {
                        parts.push(clean_name(&d.name));
                        rec = d.parent;
                        seq = d.parent_seq;
                    }
                    _ => {
                        parts.push("$Orphaned".into());
                        break;
                    }
                }
            }
            parts.reverse();
            let p = parts.join("/");
            path_cache.insert(start.0, p.clone());
            p
        };

        for d in deleted {
            let path = path_of(d.parent, d.parent_seq);
            let name = clean_name(&d.name);
            let file = match d.data {
                Data::Resident(bytes, vo) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    QuickFile {
                        name,
                        path,
                        size: bytes.len() as u64,
                        extents: vec![(d.rec_disk + vo as u64, bytes.len() as u64)],
                        inline: Some(bytes),
                        modified: d.modified,
                        created: d.created,
                        confidence: Confidence::High,
                    }
                }
                Data::NonResident {
                    runs,
                    start_vcn,
                    size,
                    flags,
                } => {
                    if size == 0 || start_vcn != 0 {
                        continue;
                    }
                    let mut all = vec![(0u64, runs)];
                    if let Some(more) = ext_data.remove(&d.rec) {
                        all.extend(more);
                        all.sort_by_key(|(v, _)| *v);
                    }
                    let mut reused = false;
                    let mut extents: Vec<(u64, u64)> = Vec::new();
                    let mut left = size;
                    for (_, runs) in &all {
                        for (lcn, len) in runs {
                            if left == 0 {
                                break;
                            }
                            let bytes = (len * cluster).min(left);
                            left -= bytes;
                            match lcn {
                                Some(l) => {
                                    let clusters = bytes.div_ceil(cluster);
                                    if !bitmap.is_empty()
                                        && (0..clusters.min(4096)).any(|c| bit_set(&bitmap, l + c))
                                    {
                                        reused = true;
                                    }
                                    extents.push((part.offset + l * cluster, bytes));
                                }
                                None => extents.push((u64::MAX, bytes)),
                            }
                        }
                    }
                    if left > 0 || extents.iter().all(|e| e.0 == u64::MAX) {
                        continue;
                    }
                    let confidence = if reused || flags & 0x4001 != 0 {
                        Confidence::Low
                    } else {
                        Confidence::High
                    };
                    QuickFile {
                        name,
                        path,
                        size,
                        extents,
                        inline: None,
                        modified: d.modified,
                        created: d.created,
                        confidence,
                    }
                }
            };
            out(file);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runlists() {
        // 0x21: len 1 byte, offset 2 bytes. len=0x18, lcn=0x5634; then sparse run len 0x10;
        // then a negative relative offset (-0x10).
        let data = [0x21, 0x18, 0x34, 0x56, 0x01, 0x10, 0x11, 0x08, 0xF0, 0x00];
        let r = decode_runs(&data).unwrap();
        assert_eq!(
            r,
            vec![(Some(0x5634), 0x18), (None, 0x10), (Some(0x5624), 0x08)]
        );
    }

    #[test]
    fn fixups() {
        let mut rec = vec![0u8; 1024];
        rec[4..6].copy_from_slice(&48u16.to_le_bytes());
        rec[6..8].copy_from_slice(&3u16.to_le_bytes());
        rec[48..50].copy_from_slice(&[0xAB, 0xCD]);
        rec[50..52].copy_from_slice(&[1, 2]);
        rec[52..54].copy_from_slice(&[3, 4]);
        rec[510..512].copy_from_slice(&[0xAB, 0xCD]);
        rec[1022..1024].copy_from_slice(&[0xAB, 0xCD]);
        assert!(apply_fixups(&mut rec));
        assert_eq!(&rec[510..512], &[1, 2]);
        assert_eq!(&rec[1022..1024], &[3, 4]);
    }
}
