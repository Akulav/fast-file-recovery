//! Image formats: JPEG, PNG, GIF, BMP, TIFF family, WebP, PSD, ICO.
//! (HEIC/AVIF use the ISO-BMFF walker in `video.rs`.)

use super::{Carved, Confidence, FileMeta, Reader};

const JPEG_MAX: u64 = 256 << 20;

/// JPEG: marker walk SOI → EOI. APPn segments (including EXIF thumbnails) are skipped by
/// length, so an embedded thumbnail's EOI does not end the file.
pub fn jpeg(r: &mut Reader, start: u64) -> Option<Carved> {
    let first = r.bytes::<2>(start + 2)?;
    if first[0] != 0xFF
        || !matches!(first[1], 0xE0..=0xEF | 0xDB | 0xC4 | 0xC0..=0xC3 | 0xFE | 0xDD | 0xFF)
    {
        return None;
    }
    let mut pos = start + 2;
    let mut meta = None;
    let mut seen_sof = false;
    let mut seen_sos = false;
    for _ in 0..100_000 {
        if pos - start > JPEG_MAX {
            return None;
        }
        if r.u8(pos)? != 0xFF {
            return None;
        }
        let mut m = r.u8(pos + 1)?;
        while m == 0xFF {
            pos += 1;
            m = r.u8(pos + 1)?;
        }
        match m {
            0xD9 => {
                if !seen_sof || !seen_sos {
                    return None;
                }
                return Some(Carved::new(pos + 2 - start, Confidence::High).meta(meta));
            }
            0xD0..=0xD7 | 0x01 => pos += 2,
            0x00 | 0xD8 => return None,
            _ => {
                let len = r.u16be(pos + 2)? as u64;
                if len < 2 {
                    return None;
                }
                if matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                    let h = r.u16be(pos + 5)? as u32;
                    let w = r.u16be(pos + 7)? as u32;
                    meta = FileMeta::dims(w, h);
                    seen_sof = true;
                }
                pos += 2 + len;
                if m == 0xDA {
                    seen_sos = true;
                    pos = scan_entropy(r, pos, start + JPEG_MAX)?;
                }
            }
        }
    }
    None
}

/// Find the next marker after entropy-coded data: 0xFF followed by a byte that is not 0x00
/// (stuffing) and not RST0-7.
fn scan_entropy(r: &mut Reader, from: u64, end: u64) -> Option<u64> {
    let mut found = None;
    r.scan(from, end.saturating_sub(from), 1, |at, piece| {
        let mut i = 0;
        while let Some(p) = memchr::memchr(0xFF, &piece[i..]) {
            let j = i + p;
            if j + 1 >= piece.len() {
                break;
            }
            let n = piece[j + 1];
            if n != 0x00 && !(0xD0..=0xD7).contains(&n) && n != 0xFF {
                found = Some(at + j as u64);
                return false;
            }
            i = j + 1;
        }
        true
    });
    found
}

/// PNG: chunk walk to IEND with CRC checks.
pub fn png(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start + 8;
    let mut meta = None;
    let mut crc_ok = true;
    let mut first = true;
    for _ in 0..1_000_000 {
        let len = r.u32be(pos)? as u64;
        let ty = r.bytes::<4>(pos + 4)?;
        if len > 0x7FFF_FFFF || !ty.iter().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        if first {
            if &ty != b"IHDR" || len != 13 {
                return None;
            }
            meta = FileMeta::dims(r.u32be(pos + 8)?, r.u32be(pos + 12)?);
            first = false;
        }
        // CRC over type + data.
        let mut h = crc32fast::Hasher::new();
        h.update(&ty);
        let mut ok = true;
        if len > 0 {
            let mut seen = 0u64;
            r.scan(pos + 8, len, 0, |_, piece| {
                h.update(piece);
                seen += piece.len() as u64;
                true
            });
            ok = seen == len;
        }
        let stored = r.u32be(pos + 8 + len)?;
        if !ok {
            return None;
        }
        if h.finalize() != stored {
            crc_ok = false;
        }
        pos += 12 + len;
        if &ty == b"IEND" {
            let conf = if crc_ok {
                Confidence::High
            } else {
                Confidence::Medium
            };
            return Some(Carved::new(pos - start, conf).meta(meta));
        }
    }
    None
}

/// GIF: block walk to the trailer (`;`).
pub fn gif(r: &mut Reader, start: u64) -> Option<Carved> {
    let w = r.u16le(start + 6)? as u32;
    let h = r.u16le(start + 8)? as u32;
    let flags = r.u8(start + 10)?;
    let mut pos = start + 13;
    if flags & 0x80 != 0 {
        pos += 3 * (1u64 << ((flags & 7) + 1));
    }
    let mut frames = 0u32;
    for _ in 0..1_000_000 {
        match r.u8(pos)? {
            0x3B => {
                if frames == 0 {
                    return None;
                }
                return Some(
                    Carved::new(pos + 1 - start, Confidence::High).meta(FileMeta::dims(w, h)),
                );
            }
            0x21 => {
                pos = skip_sub_blocks(r, pos + 2)?;
            }
            0x2C => {
                let lf = r.u8(pos + 9)?;
                pos += 10;
                if lf & 0x80 != 0 {
                    pos += 3 * (1u64 << ((lf & 7) + 1));
                }
                let min_code = r.u8(pos)?;
                if !(1..=12).contains(&min_code) {
                    return None;
                }
                pos = skip_sub_blocks(r, pos + 1)?;
                frames += 1;
            }
            _ => return None,
        }
    }
    None
}

fn skip_sub_blocks(r: &mut Reader, mut pos: u64) -> Option<u64> {
    for _ in 0..10_000_000 {
        let n = r.u8(pos)? as u64;
        pos += 1 + n;
        if n == 0 {
            return Some(pos);
        }
    }
    None
}

/// BMP: size from the header, validated against the DIB header.
pub fn bmp(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<30>(start)?;
    let size = u32::from_le_bytes([h[2], h[3], h[4], h[5]]) as u64;
    let reserved = u32::from_le_bytes([h[6], h[7], h[8], h[9]]);
    let data_off = u32::from_le_bytes([h[10], h[11], h[12], h[13]]) as u64;
    let dib = u32::from_le_bytes([h[14], h[15], h[16], h[17]]);
    if reserved != 0 || !matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124) {
        return None;
    }
    if data_off < 14 + dib as u64 || data_off >= size {
        return None;
    }
    let (w, ht, planes, bpp) = if dib == 12 {
        (
            u16::from_le_bytes([h[18], h[19]]) as i64,
            u16::from_le_bytes([h[20], h[21]]) as i64,
            u16::from_le_bytes([h[22], h[23]]),
            u16::from_le_bytes([h[24], h[25]]),
        )
    } else {
        (
            i32::from_le_bytes([h[18], h[19], h[20], h[21]]) as i64,
            i32::from_le_bytes([h[22], h[23], h[24], h[25]]) as i64,
            u16::from_le_bytes([h[26], h[27]]),
            u16::from_le_bytes([h[28], h[29]]),
        )
    };
    if planes != 1 || !matches!(bpp, 1 | 2 | 4 | 8 | 16 | 24 | 32 | 0) || w <= 0 || ht == 0 {
        return None;
    }
    let ht = ht.unsigned_abs();
    // Uncompressed sanity check: pixel data must fit.
    if dib >= 40 && bpp >= 8 {
        let compression = r.u32le(start + 30)?;
        if compression == 0 {
            let row = ((w as u64 * bpp as u64).div_ceil(32)) * 4;
            if data_off + row * ht > size + 4 {
                return None;
            }
        }
    }
    Some(Carved::new(size, Confidence::Medium).meta(FileMeta::dims(w as u32, ht as u32)))
}

/// TIFF and raw formats built on it (DNG, CR2, NEF, ARW): IFD walk to the furthest strip,
/// tile or tag data.
pub fn tiff(r: &mut Reader, start: u64) -> Option<Carved> {
    let le = match r.bytes::<4>(start)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let rd16 = |r: &mut Reader, o: u64| if le { r.u16le(o) } else { r.u16be(o) };
    let rd32 = |r: &mut Reader, o: u64| if le { r.u32le(o) } else { r.u32be(o) };
    let first = rd32(r, start + 4)? as u64;
    if first < 8 {
        return None;
    }
    let mut max_end = 8u64;
    let mut queue = vec![first];
    let mut visited = Vec::new();
    let mut ifds = 0;
    let mut has_image_data = false;
    let mut ext = "tif";
    let mut make = String::new();
    let (mut width, mut height) = (0u32, 0u32);
    if r.bytes::<2>(start + 8)? == *b"CR" {
        ext = "cr2";
    }
    while let Some(ifd) = queue.pop() {
        if ifds >= 64 || visited.contains(&ifd) || ifd > 1 << 32 {
            continue;
        }
        visited.push(ifd);
        ifds += 1;
        let at = start + ifd;
        let count = rd16(r, at)? as u64;
        if count == 0 || count > 1000 {
            if ifds == 1 {
                return None;
            }
            continue;
        }
        max_end = max_end.max(ifd + 2 + count * 12 + 4);
        let mut offsets: Vec<u64> = Vec::new();
        let mut counts: Vec<u64> = Vec::new();
        for i in 0..count {
            let e = at + 2 + i * 12;
            let tag = rd16(r, e)?;
            let ty = rd16(r, e + 2)?;
            let n = rd32(r, e + 4)? as u64;
            let tsize: u64 = match ty {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 | 16 | 17 | 18 => 8,
                _ => {
                    if ifds == 1 && i == 0 {
                        return None;
                    }
                    continue;
                }
            };
            let bytes = n.saturating_mul(tsize);
            let value_off = if bytes > 4 {
                let o = rd32(r, e + 8)? as u64;
                max_end = max_end.max(o + bytes);
                o
            } else {
                e + 8 - start
            };
            let read_vals = |r: &mut Reader, max: u64| -> Vec<u64> {
                let mut v = Vec::new();
                for k in 0..n.min(max) {
                    let o = start + value_off + k * tsize;
                    let x = match tsize {
                        2 => rd16(r, o).map(|x| x as u64),
                        4 => rd32(r, o).map(|x| x as u64),
                        _ => None,
                    };
                    match x {
                        Some(x) => v.push(x),
                        None => break,
                    }
                }
                v
            };
            match tag {
                256 => width = read_vals(r, 1).first().copied().unwrap_or(0) as u32,
                257 => height = read_vals(r, 1).first().copied().unwrap_or(0) as u32,
                271 => {
                    if let Some(v) = r.vec(start + value_off, n.min(32) as usize) {
                        make = String::from_utf8_lossy(&v).to_ascii_uppercase();
                    }
                }
                273 | 324 => offsets = read_vals(r, 100_000),
                279 | 325 => counts = read_vals(r, 100_000),
                // JPEG interchange format (thumbnails / old-style JPEG).
                513 => offsets = read_vals(r, 1),
                514 => counts = read_vals(r, 1),
                330 | 34665 | 34853 => {
                    for o in read_vals(r, 16) {
                        queue.push(o);
                    }
                }
                50706 => ext = "dng",
                _ => {}
            }
        }
        for (o, c) in offsets.iter().zip(counts.iter()) {
            if *o > 0 && *c > 0 {
                max_end = max_end.max(o + c);
                has_image_data = true;
            }
        }
        let next = rd32(r, at + 2 + count * 12)? as u64;
        if next != 0 {
            queue.push(next);
        }
    }
    if ext == "tif" {
        if make.starts_with("NIKON") {
            ext = "nef";
        } else if make.starts_with("SONY") {
            ext = "arw";
        } else if make.starts_with("CANON") {
            ext = "cr2";
        }
    }
    if !has_image_data {
        return None;
    }
    Some(
        Carved::new(max_end, Confidence::Medium)
            .ext(ext)
            .meta(FileMeta::dims(width, height)),
    )
}

/// WebP: RIFF size.
pub fn webp(r: &mut Reader, start: u64) -> Option<Carved> {
    if r.bytes::<4>(start + 8)? != *b"WEBP" {
        return None;
    }
    let chunk = r.bytes::<4>(start + 12)?;
    if !matches!(&chunk, b"VP8 " | b"VP8L" | b"VP8X") {
        return None;
    }
    let len = r.u32le(start + 4)? as u64 + 8;
    Some(Carved::new(len + (len & 1), Confidence::High))
}

/// PSD / PSB: header + section lengths + image data.
pub fn psd(r: &mut Reader, start: u64) -> Option<Carved> {
    let version = r.u16be(start + 4)?;
    if !(version == 1 || version == 2) || r.bytes::<6>(start + 6)? != [0; 6] {
        return None;
    }
    let channels = r.u16be(start + 12)? as u64;
    let h = r.u32be(start + 14)? as u64;
    let w = r.u32be(start + 18)? as u64;
    let depth = r.u16be(start + 22)? as u64;
    if !(1..=56).contains(&channels) || h == 0 || w == 0 || !matches!(depth, 1 | 8 | 16 | 32) {
        return None;
    }
    let mut pos = start + 26;
    pos += 4 + r.u32be(pos)? as u64; // colour mode data
    pos += 4 + r.u32be(pos)? as u64; // image resources
    pos += if version == 1 {
        4 + r.u32be(pos)? as u64
    } else {
        8 + r.u64be(pos)?
    }; // layer and mask info
    let compression = r.u16be(pos)?;
    pos += 2;
    match compression {
        0 => pos += channels * h * (w * depth).div_ceil(8),
        1 => {
            let rows = channels * h;
            if rows > 4_000_000 {
                return None;
            }
            let entry = if version == 1 { 2 } else { 4 };
            let mut total = 0u64;
            let table = r.vec(pos, (rows * entry) as usize)?;
            for c in table.chunks_exact(entry as usize) {
                total += if entry == 2 {
                    u16::from_be_bytes([c[0], c[1]]) as u64
                } else {
                    u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as u64
                };
            }
            pos += rows * entry + total;
        }
        _ => return None,
    }
    Some(
        Carved::new(pos - start, Confidence::Medium)
            .ext(if version == 2 { "psb" } else { "psd" })
            .meta(FileMeta::dims(w as u32, h as u32)),
    )
}

/// ICO: directory entries → furthest image end.
pub fn ico(r: &mut Reader, start: u64) -> Option<Carved> {
    let count = r.u16le(start + 4)? as u64;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut end = 6 + count * 16;
    for i in 0..count {
        let e = r.bytes::<16>(start + 6 + i * 16)?;
        let planes = u16::from_le_bytes([e[4], e[5]]);
        let bpp = u16::from_le_bytes([e[6], e[7]]);
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as u64;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as u64;
        if e[3] != 0 || planes > 1 || !matches!(bpp, 0 | 1 | 4 | 8 | 16 | 24 | 32) {
            return None;
        }
        if size == 0 || off < 6 + count * 16 || size > 16 << 20 {
            return None;
        }
        // Each image is a PNG or a BITMAPINFOHEADER (size 40).
        let head = r.bytes::<4>(start + off)?;
        if head != [0x89, b'P', b'N', b'G'] && head != [40, 0, 0, 0] {
            return None;
        }
        end = end.max(off + size);
    }
    Some(Carved::new(end, Confidence::Medium))
}
