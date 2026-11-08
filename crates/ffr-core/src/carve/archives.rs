//! Archives: RAR4/RAR5, 7z, GZIP, BZIP2, XZ, TAR, ISO 9660, CAB. (ZIP is in `documents.rs`.)

use super::{Carved, Confidence, FileMeta, Reader};
use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

// ---------------------------------------------------------------- RAR

pub fn rar4(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start + 7;
    for _ in 0..5_000_000 {
        let h = r.bytes::<7>(pos)?;
        let ty = h[2];
        let flags = u16::from_le_bytes([h[3], h[4]]);
        let hsize = u16::from_le_bytes([h[5], h[6]]) as u64;
        if !(0x72..=0x7B).contains(&ty) || hsize < 7 {
            return None;
        }
        let mut total = hsize;
        if flags & 0x8000 != 0 || matches!(ty, 0x74 | 0x7A) {
            total += r.u32le(pos + 7)? as u64;
            if ty == 0x74 && flags & 0x100 != 0 {
                total += (r.u32le(pos + 32)? as u64) << 32;
            }
        }
        pos += total;
        if ty == 0x7B {
            return Some(Carved::new(pos - start, Confidence::High).ext("rar"));
        }
    }
    None
}

fn vint(r: &mut Reader, pos: u64) -> Option<(u64, u64)> {
    let mut v = 0u64;
    for i in 0..10 {
        let b = r.u8(pos + i)?;
        v |= ((b & 0x7F) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

pub fn rar5(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start + 8;
    for _ in 0..5_000_000 {
        let (hsize, hl) = vint(r, pos + 4)?;
        if hsize == 0 || hsize > 2 << 20 {
            return None;
        }
        let mut p = pos + 4 + hl;
        let (ty, l) = vint(r, p)?;
        p += l;
        let (flags, l) = vint(r, p)?;
        p += l;
        if !(1..=5).contains(&ty) {
            return None;
        }
        if flags & 1 != 0 {
            let (_, l) = vint(r, p)?;
            p += l;
        }
        let data = if flags & 2 != 0 { vint(r, p)?.0 } else { 0 };
        pos += 4 + hl + hsize + data;
        if ty == 5 {
            return Some(Carved::new(pos - start, Confidence::High).ext("rar"));
        }
    }
    None
}

// ---------------------------------------------------------------- 7z

pub fn seven_zip(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<32>(start)?;
    if h[6] != 0 {
        return None;
    }
    let crc = u32::from_le_bytes([h[8], h[9], h[10], h[11]]);
    if crc32fast::hash(&h[12..32]) != crc {
        return None;
    }
    let off = u64::from_le_bytes(h[12..20].try_into().ok()?);
    let size = u64::from_le_bytes(h[20..28].try_into().ok()?);
    if size == 0 {
        return None;
    }
    Some(Carved::new(
        32u64.checked_add(off)?.checked_add(size)?,
        Confidence::High,
    ))
}

// ---------------------------------------------------------------- GZIP

/// GZIP: parse the header, inflate the deflate stream to find its end, then the 8-byte trailer.
pub fn gzip(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<10>(start)?;
    let flags = h[3];
    if flags & 0xE0 != 0 || !matches!(h[8], 0 | 2 | 4) {
        return None;
    }
    let mut pos = start + 10;
    if flags & 4 != 0 {
        pos += 2 + r.u16le(pos)? as u64;
    }
    let mut name = None;
    if flags & 8 != 0 {
        let e = r.find(pos, 4096, &[0])?;
        name = r
            .vec(pos, (e - pos) as usize)
            .map(|v| String::from_utf8_lossy(&v).into_owned());
        pos = e + 1;
    }
    if flags & 16 != 0 {
        pos = r.find(pos, 65_536, &[0])? + 1;
    }
    if flags & 2 != 0 {
        pos += 2;
    }
    let mut d = DecompressorOxide::new();
    let mut out = vec![0u8; 1 << 16];
    let mut out_pos = 0usize;
    let mut total_out = 0u64;
    let mut inp = vec![0u8; 1 << 16];
    let mut in_pos = pos;
    let mut buf_len = 0usize;
    let mut buf_off = 0usize;
    for _ in 0..1_000_000 {
        if buf_off == buf_len {
            let avail = (r.size().saturating_sub(in_pos)).min(inp.len() as u64) as usize;
            if avail == 0 || !r.read(in_pos, &mut inp[..avail]) {
                return None;
            }
            buf_len = avail;
            buf_off = 0;
        }
        let flags = inflate_flags::TINFL_FLAG_HAS_MORE_INPUT;
        let (status, consumed, written) =
            decompress(&mut d, &inp[buf_off..buf_len], &mut out, out_pos, flags);
        buf_off += consumed;
        in_pos += consumed as u64;
        total_out += written as u64;
        out_pos = (out_pos + written) & (out.len() - 1);
        match status {
            TINFLStatus::Done => {
                let isize = r.u32le(in_pos + 4)?;
                let conf = if isize as u64 == total_out & 0xFFFF_FFFF {
                    Confidence::High
                } else {
                    Confidence::Medium
                };
                return Some(
                    Carved::new(in_pos + 8 - start, conf).meta(name.and_then(FileMeta::title)),
                );
            }
            TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput => {
                if r.over_limit() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    None
}

// ---------------------------------------------------------------- BZIP2

/// BZIP2: find the bit-aligned end-of-stream marker (0x177245385090) + 32-bit CRC.
pub fn bzip2(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<10>(start)?;
    if !(b'1'..=b'9').contains(&h[3]) || h[4..10] != [0x31, 0x41, 0x59, 0x26, 0x53, 0x59] {
        return None;
    }
    const EOS: u64 = 0x1772_4538_5090;
    let mut window: u64 = 0;
    let mut bits: u64 = 0;
    let mut end = None;
    r.scan(start + 4, 1 << 32, 0, |at, piece| {
        for (i, &b) in piece.iter().enumerate() {
            window = (window << 8) | b as u64;
            bits += 8;
            if bits < 56 {
                continue;
            }
            for k in 0..8 {
                if (window >> k) & 0xFFFF_FFFF_FFFF == EOS {
                    // Marker ends k bits before the end of this byte; then a 32-bit CRC.
                    let marker_end_bit = (at + i as u64 + 1 - start) * 8 - k;
                    end = Some((marker_end_bit + 32).div_ceil(8));
                    return false;
                }
            }
        }
        true
    });
    end.map(|e| Carved::new(e, Confidence::High))
}

// ---------------------------------------------------------------- XZ

pub fn xz(r: &mut Reader, start: u64) -> Option<Carved> {
    let flags = r.bytes::<2>(start + 6)?;
    let crc = r.u32le(start + 8)?;
    if crc32fast::hash(&flags) != crc || flags[0] != 0 || flags[1] > 0x0F {
        return None;
    }
    let mut from = start + 12;
    for _ in 0..100_000 {
        let p = r.find(from, (1u64 << 32).saturating_sub(from - start), b"YZ")?;
        from = p + 1;
        if !(p + 2 - start).is_multiple_of(4) || p < start + 22 {
            continue;
        }
        let footer = r.bytes::<12>(p - 10)?;
        if footer[8..10] != flags {
            continue;
        }
        let fcrc = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
        if crc32fast::hash(&footer[4..10]) == fcrc {
            return Some(Carved::new(p + 2 - start, Confidence::High));
        }
    }
    None
}

// ---------------------------------------------------------------- TAR

fn octal(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|&b| b & 0x80 != 0) {
        // GNU base-256.
        let mut v = 0u64;
        for &b in &field[1..] {
            v = v.checked_shl(8)? | b as u64;
        }
        return Some(v);
    }
    let s: String = field
        .iter()
        .take_while(|&&c| c != 0 && c != b' ')
        .skip_while(|&&c| c == b' ')
        .map(|&c| c as char)
        .collect();
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(&s, 8).ok()
}

fn tar_header_ok(h: &[u8]) -> bool {
    let Some(stored) = octal(&h[148..156]) else {
        return false;
    };
    let sum: u64 = h
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                32
            } else {
                b as u64
            }
        })
        .sum();
    sum == stored
}

pub fn tar(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start;
    let mut entries = 0u64;
    for _ in 0..10_000_000 {
        let Some(h) = r.vec(pos, 512) else { break };
        if h.iter().all(|&b| b == 0) {
            if entries == 0 {
                return None;
            }
            let mut end = pos + 512;
            if r.vec(end, 512).is_some_and(|z| z.iter().all(|&b| b == 0)) {
                end += 512;
            }
            return Some(Carved::new(end - start, Confidence::High));
        }
        if !tar_header_ok(&h) {
            if entries == 0 {
                return None;
            }
            break;
        }
        let size = octal(&h[124..136])?;
        let ty = h[156];
        let data = if matches!(ty, b'1' | b'2' | b'3' | b'4' | b'5' | b'6') {
            0
        } else {
            size
        };
        pos += 512 + data.div_ceil(512) * 512;
        entries += 1;
    }
    (entries > 0).then(|| Carved::new(pos - start, Confidence::Medium))
}

// ---------------------------------------------------------------- ISO 9660

pub fn iso(r: &mut Reader, start: u64) -> Option<Carved> {
    let pvd = start + 0x8000;
    let blocks = r.u32le(pvd + 80)? as u64;
    let blocks_be = r.u32be(pvd + 84)? as u64;
    let bs = r.u16le(pvd + 128)? as u64;
    if blocks != blocks_be || !matches!(bs, 512 | 1024 | 2048) || blocks < 16 {
        return None;
    }
    let label = r
        .vec(pvd + 40, 32)
        .map(|v| String::from_utf8_lossy(&v).trim().to_string());
    Some(Carved::new(blocks * bs, Confidence::High).meta(label.and_then(FileMeta::title)))
}

// ---------------------------------------------------------------- CAB

pub fn cab(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<36>(start)?;
    let size = u32::from_le_bytes([h[8], h[9], h[10], h[11]]) as u64;
    let first_file = u32::from_le_bytes([h[16], h[17], h[18], h[19]]) as u64;
    if h[4..8] != [0; 4] || h[24] != 3 || h[25] != 1 || first_file >= size || first_file < 36 {
        return None;
    }
    let folders = u16::from_le_bytes([h[26], h[27]]);
    let files = u16::from_le_bytes([h[28], h[29]]);
    if folders == 0 || files == 0 {
        return None;
    }
    Some(Carved::new(size, Confidence::High))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_fields() {
        assert_eq!(octal(b"00000001750\0"), Some(1000));
        assert_eq!(octal(b"           \0"), Some(0));
        assert_eq!(octal(&[0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]), Some(256));
    }
}
