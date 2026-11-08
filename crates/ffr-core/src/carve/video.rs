//! Video formats: ISO-BMFF (MP4/MOV/M4V/3GP, also used for M4A/HEIC/AVIF/CR3), AVI, MKV/WebM,
//! FLV, MPEG-TS, WMV/ASF.

use super::{is_fourcc, Carved, Confidence, FileMeta, Reader};

// ---------------------------------------------------------------- ISO-BMFF

const BMFF_TOP: &[&[u8; 4]] = &[
    b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"uuid", b"meta", b"pdin", b"moof",
    b"mfra", b"sidx", b"styp", b"prft", b"emsg", b"ssix", b"pnot", b"junk", b"idat", b"iloc",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum BmffClass {
    Video(&'static str),
    Audio,
    Image(&'static str),
}

struct BmffInfo {
    len: u64,
    class: BmffClass,
    moov: bool,
    mdat: bool,
    meta: bool,
    duration_ms: u64,
}

fn classify(major: &[u8; 4], compat: &[[u8; 4]]) -> BmffClass {
    let has = |b: &[u8; 4]| major == b || compat.contains(b);
    if has(b"avif") || has(b"avis") {
        return BmffClass::Image("avif");
    }
    if [
        b"heic", b"heix", b"hevc", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1",
    ]
    .contains(&major)
    {
        return BmffClass::Image("heic");
    }
    if major == b"crx " {
        return BmffClass::Image("cr3");
    }
    if [b"M4A ", b"M4B ", b"M4P ", b"F4A "].contains(&major) {
        return BmffClass::Audio;
    }
    if major == b"qt  " {
        return BmffClass::Video("mov");
    }
    if major.starts_with(b"3gp") || major.starts_with(b"3g2") {
        return BmffClass::Video("3gp");
    }
    if major.starts_with(b"M4V") {
        return BmffClass::Video("m4v");
    }
    BmffClass::Video("mp4")
}

fn bmff(r: &mut Reader, start: u64) -> Option<BmffInfo> {
    let ftyp_size = r.u32be(start)? as u64;
    if r.bytes::<4>(start + 4)? != *b"ftyp"
        || !(16..=1024).contains(&ftyp_size)
        || !ftyp_size.is_multiple_of(4)
    {
        return None;
    }
    let major = r.bytes::<4>(start + 8)?;
    if !is_fourcc(&major) {
        return None;
    }
    let mut compat = Vec::new();
    let mut p = start + 16;
    while p + 4 <= start + ftyp_size && compat.len() < 64 {
        let b = r.bytes::<4>(p)?;
        if !is_fourcc(&b) && b != [0; 4] {
            return None;
        }
        compat.push(b);
        p += 4;
    }
    let class = classify(&major, &compat);
    let mut info = BmffInfo {
        len: ftyp_size,
        class,
        moov: false,
        mdat: false,
        meta: false,
        duration_ms: 0,
    };
    let mut pos = start + ftyp_size;
    for _ in 0..100_000 {
        let Some(size32) = r.u32be(pos) else { break };
        let Some(ty) = r.bytes::<4>(pos + 4) else {
            break;
        };
        if !is_fourcc(&ty) {
            break;
        }
        let required_seen = match class {
            BmffClass::Image(_) => info.meta,
            _ => info.moov && info.mdat,
        };
        if required_seen && !BMFF_TOP.contains(&&ty) {
            break;
        }
        let size = match size32 {
            0 => break,
            1 => match r.u64be(pos + 8) {
                Some(s) if s >= 16 => s,
                _ => break,
            },
            s if s < 8 => break,
            s => s as u64,
        };
        match &ty {
            b"moov" => {
                info.moov = true;
                info.duration_ms = mvhd_duration(r, pos, size).unwrap_or(0);
            }
            b"mdat" => info.mdat = true,
            b"meta" => info.meta = true,
            _ => {}
        }
        pos = pos.checked_add(size)?;
        info.len = pos - start;
        if pos >= r.size() {
            break;
        }
    }
    Some(info)
}

/// Duration from `moov/mvhd` when it is the first child (the common layout).
fn mvhd_duration(r: &mut Reader, moov: u64, size: u64) -> Option<u64> {
    if size < 40 {
        return None;
    }
    let child = moov + 8;
    if r.bytes::<4>(child + 4)? != *b"mvhd" {
        return None;
    }
    let version = r.u8(child + 8)?;
    let (scale, dur) = if version == 1 {
        (r.u32be(child + 28)? as u64, r.u64be(child + 32)?)
    } else {
        (r.u32be(child + 20)? as u64, r.u32be(child + 24)? as u64)
    };
    (scale > 0).then(|| dur.saturating_mul(1000) / scale)
}

pub fn bmff_video(r: &mut Reader, start: u64) -> Option<Carved> {
    let i = bmff(r, start)?;
    let BmffClass::Video(ext) = i.class else {
        return None;
    };
    let conf = match (i.moov, i.mdat) {
        (true, true) => Confidence::High,
        (false, false) => return None,
        _ => Confidence::Low,
    };
    Some(
        Carved::new(i.len, conf)
            .ext(ext)
            .meta(FileMeta::duration(i.duration_ms)),
    )
}

pub fn bmff_audio(r: &mut Reader, start: u64) -> Option<Carved> {
    let i = bmff(r, start)?;
    if i.class != BmffClass::Audio {
        return None;
    }
    let conf = match (i.moov, i.mdat) {
        (true, true) => Confidence::High,
        (false, false) => return None,
        _ => Confidence::Low,
    };
    Some(
        Carved::new(i.len, conf)
            .ext("m4a")
            .meta(FileMeta::duration(i.duration_ms)),
    )
}

pub fn bmff_image(r: &mut Reader, start: u64) -> Option<Carved> {
    let i = bmff(r, start)?;
    let BmffClass::Image(ext) = i.class else {
        return None;
    };
    if ext == "cr3" {
        if !i.moov {
            return None;
        }
    } else if !i.meta {
        return None;
    }
    let conf = if i.mdat || ext != "cr3" && i.meta {
        Confidence::High
    } else {
        Confidence::Medium
    };
    Some(Carved::new(i.len, conf).ext(ext))
}

// ---------------------------------------------------------------- AVI

/// AVI: RIFF size, plus any OpenDML `RIFF....AVIX` extension chunks that follow.
pub fn avi(r: &mut Reader, start: u64) -> Option<Carved> {
    if r.bytes::<4>(start + 8)? != *b"AVI " || r.bytes::<4>(start + 12)? != *b"LIST" {
        return None;
    }
    let mut len = r.u32le(start + 4)? as u64 + 8;
    len += len & 1;
    for _ in 0..1024 {
        let p = start + len;
        match (r.bytes::<4>(p), r.bytes::<4>(p + 8)) {
            (Some(riff), Some(avix)) if &riff == b"RIFF" && &avix == b"AVIX" => {
                let l = r.u32le(p + 4)? as u64 + 8;
                len += l + (l & 1);
            }
            _ => break,
        }
    }
    Some(Carved::new(len, Confidence::High))
}

// ---------------------------------------------------------------- Matroska / WebM

fn ebml_id(r: &mut Reader, pos: u64) -> Option<(u32, u64)> {
    let b = r.u8(pos)?;
    let len = b.leading_zeros() as u64 + 1;
    if len > 4 {
        return None;
    }
    let mut id = b as u32;
    for i in 1..len {
        id = (id << 8) | r.u8(pos + i)? as u32;
    }
    Some((id, len))
}

/// Returns (size, length of the size field). `None` size means "unknown".
fn ebml_size(r: &mut Reader, pos: u64) -> Option<(Option<u64>, u64)> {
    let b = r.u8(pos)?;
    let len = b.leading_zeros() as u64 + 1;
    if len > 8 {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu8 >> len };
    let mut v = (b & mask) as u64;
    let mut all_ones = (b & mask) == mask;
    for i in 1..len {
        let x = r.u8(pos + i)?;
        all_ones &= x == 0xFF;
        v = (v << 8) | x as u64;
    }
    Some((if all_ones { None } else { Some(v) }, len))
}

const MKV_LEVEL1: &[u32] = &[
    0x114D_9B74, // SeekHead
    0x1549_A966, // Info
    0x1654_AE6B, // Tracks
    0x1F43_B675, // Cluster
    0x1C53_BB6B, // Cues
    0x1941_A469, // Attachments
    0x1043_A770, // Chapters
    0x1254_C367, // Tags
    0xEC,        // Void
    0xBF,        // CRC-32
];

pub fn mkv(r: &mut Reader, start: u64) -> Option<Carved> {
    let (hs, hl) = ebml_size(r, start + 4)?;
    let hs = hs?;
    if hs > 4096 {
        return None;
    }
    let hdr_start = start + 4 + hl;
    let hdr_end = hdr_start + hs;
    // DocType
    let mut ext = "mkv";
    let mut p = hdr_start;
    let mut found_doctype = false;
    while p < hdr_end {
        let (id, il) = ebml_id(r, p)?;
        let (sz, sl) = ebml_size(r, p + il)?;
        let sz = sz?;
        if id == 0x4282 {
            let v = r.vec(p + il + sl, sz.min(32) as usize)?;
            let s = String::from_utf8_lossy(&v);
            ext = if s.starts_with("webm") {
                "webm"
            } else if s.starts_with("matroska") {
                "mkv"
            } else {
                return None;
            };
            found_doctype = true;
        }
        p += il + sl + sz;
    }
    if !found_doctype {
        return None;
    }
    let (id, il) = ebml_id(r, hdr_end)?;
    if id != 0x1853_8067 {
        return None;
    }
    let (seg, sl) = ebml_size(r, hdr_end + il)?;
    let data = hdr_end + il + sl;
    if let Some(seg) = seg {
        return Some(Carved::new(data + seg - start, Confidence::High).ext(ext));
    }
    // Unknown-size segment (live recording): walk level-1 children.
    let mut pos = data;
    for _ in 0..1_000_000 {
        let Some((id, il)) = ebml_id(r, pos) else {
            break;
        };
        if !MKV_LEVEL1.contains(&id) {
            break;
        }
        let Some((sz, sl)) = ebml_size(r, pos + il) else {
            break;
        };
        match sz {
            Some(s) => pos += il + sl + s,
            None if id == 0x1F43_B675 => {
                // Cluster of unknown size: walk its children until a level-1 id appears.
                let mut c = pos + il + sl;
                while let Some((cid, cil)) = ebml_id(r, c) {
                    if MKV_LEVEL1.contains(&cid) && cid != 0xEC && cid != 0xBF {
                        break;
                    }
                    if !matches!(cid, 0xE7 | 0xA3 | 0xA0 | 0xA7 | 0xAB | 0x5854 | 0xEC | 0xBF) {
                        break;
                    }
                    let Some((Some(cs), csl)) = ebml_size(r, c + cil) else {
                        break;
                    };
                    c += cil + csl + cs;
                }
                pos = c;
            }
            None => break,
        }
    }
    (pos > data).then(|| Carved::new(pos - start, Confidence::Medium).ext(ext))
}

// ---------------------------------------------------------------- FLV

pub fn flv(r: &mut Reader, start: u64) -> Option<Carved> {
    let flags = r.u8(start + 4)?;
    let hsize = r.u32be(start + 5)? as u64;
    if flags & !0x05 != 0 || hsize != 9 || r.u32be(start + 9)? != 0 {
        return None;
    }
    let mut pos = start + 13;
    let mut tags = 0u32;
    let mut last_ts = 0u32;
    for _ in 0..50_000_000 {
        let Some(h) = r.bytes::<11>(pos) else { break };
        if !matches!(h[0] & 0x1F, 8 | 9 | 18) || h[8..11] != [0, 0, 0] {
            break;
        }
        let size = u32::from_be_bytes([0, h[1], h[2], h[3]]) as u64;
        let Some(prev) = r.u32be(pos + 11 + size) else {
            break;
        };
        if prev as u64 != 11 + size {
            break;
        }
        last_ts = u32::from_be_bytes([h[7], h[4], h[5], h[6]]);
        pos += 15 + size;
        tags += 1;
    }
    (tags > 0).then(|| {
        Carved::new(pos - start, Confidence::Medium).meta(FileMeta::duration(last_ts as u64))
    })
}

// ---------------------------------------------------------------- MPEG-TS

/// MPEG transport stream: a run of 188-byte packets each starting with 0x47. The first packet
/// must be the PAT (PID 0, payload start) to keep noise down.
pub fn mpeg_ts(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<4>(start)?;
    let pid = (((h[1] & 0x1F) as u16) << 8) | h[2] as u16;
    if pid != 0 || h[1] & 0x40 == 0 || h[1] & 0x80 != 0 {
        return None;
    }
    let mut packets = 0u64;
    let mut ok = true;
    r.scan(start, u64::MAX, 0, |at, piece| {
        let base = (at - start) % 188;
        let mut i = if base == 0 { 0 } else { (188 - base) as usize };
        while i < piece.len() {
            if piece[i] != 0x47 {
                ok = false;
                return false;
            }
            packets += 1;
            i += 188;
        }
        true
    });
    let _ = ok;
    (packets >= 8).then(|| Carved::new(packets * 188, Confidence::Medium))
}

// ---------------------------------------------------------------- ASF / WMV / WMA

const ASF_FILE_PROPS: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const ASF_DATA: [u8; 16] = [
    0x36, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_VIDEO_MEDIA: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

pub fn asf(r: &mut Reader, start: u64) -> Option<Carved> {
    let hsize = r.u64le(start + 16)?;
    let nobj = r.u32le(start + 24)?;
    if !(30..=16 << 20).contains(&hsize) || nobj == 0 || nobj > 1000 {
        return None;
    }
    let header = r.vec(start, hsize as usize)?;
    let mut file_size = None;
    let mut duration = 0u64;
    let mut p = 30usize;
    for _ in 0..nobj {
        let guid = header.get(p..p + 16)?;
        let size = crate::util::le64(&header, p + 16)? as usize;
        if size < 24 || p + size > header.len() {
            break;
        }
        if guid == ASF_FILE_PROPS {
            file_size = crate::util::le64(&header, p + 40);
            // play duration in 100 ns units at +64, preroll (ms) at +80
            let play = crate::util::le64(&header, p + 64).unwrap_or(0);
            let preroll = crate::util::le64(&header, p + 80).unwrap_or(0);
            duration = (play / 10_000).saturating_sub(preroll);
        }
        p += size;
    }
    let video = memchr::memmem::find(&header, &ASF_VIDEO_MEDIA).is_some();
    let ext = if video { "wmv" } else { "wma" };
    let data_guid = r.bytes::<16>(start + hsize)?;
    if data_guid != ASF_DATA {
        return None;
    }
    let data_size = r.u64le(start + hsize + 16)?;
    let min = hsize + data_size;
    let len = match file_size {
        Some(fs) if fs >= min => fs,
        _ => min,
    };
    Some(
        Carved::new(len, Confidence::High)
            .ext(ext)
            .meta(FileMeta::duration(duration)),
    )
}
