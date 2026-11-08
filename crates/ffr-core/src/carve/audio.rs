//! Audio formats: MP3 (ID3v2 + frame walk), WAV, FLAC, OGG, AAC ADTS, AIFF, MIDI.

use super::{Carved, Confidence, FileMeta, Reader};

// ---------------------------------------------------------------- MP3

const BR_V1: [[u32; 15]; 3] = [
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
];
const BR_V2: [[u32; 15]; 2] = [
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// Parse an MPEG audio frame header → (frame length, samples, sample rate, id bits).
fn mpeg_frame(h: [u8; 4]) -> Option<(u64, u32, u32, u8)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let ver = (h[1] >> 3) & 3;
    let layer = (h[1] >> 1) & 3;
    if ver == 1 || layer == 0 {
        return None;
    }
    let bi = (h[2] >> 4) as usize;
    let si = ((h[2] >> 2) & 3) as usize;
    if bi == 0 || bi == 15 || si == 3 {
        return None;
    }
    let pad = ((h[2] >> 1) & 1) as u64;
    let sr = match ver {
        3 => [44_100, 48_000, 32_000][si],
        2 => [22_050, 24_000, 16_000][si],
        _ => [11_025, 12_000, 8_000][si],
    };
    let br = if ver == 3 {
        BR_V1[(3 - layer) as usize][bi]
    } else if layer == 3 {
        BR_V2[0][bi]
    } else {
        BR_V2[1][bi]
    } as u64
        * 1000;
    let sr64 = sr as u64;
    let (len, samples) = match layer {
        3 => ((12 * br / sr64 + pad) * 4, 384),
        2 => (144 * br / sr64 + pad, 1152),
        _ if ver == 3 => (144 * br / sr64 + pad, 1152),
        _ => (72 * br / sr64 + pad, 576),
    };
    if len < 21 {
        return None;
    }
    Some((len, samples, sr, h[1] & 0xFE))
}

/// Walk MPEG frames from `pos`. Returns (end, frames, duration ms).
fn walk_mpeg(r: &mut Reader, mut pos: u64) -> (u64, u32, u64) {
    let mut frames = 0u32;
    let mut samples = 0u64;
    let mut first: Option<(u32, u8)> = None;
    while frames < 2_000_000 {
        let Some(h) = r.bytes::<4>(pos) else { break };
        let Some((len, s, sr, id)) = mpeg_frame(h) else {
            break;
        };
        match first {
            None => first = Some((sr, id)),
            Some(f) if f != (sr, id) => break,
            _ => {}
        }
        pos += len;
        frames += 1;
        samples += s as u64;
    }
    let ms = first.map(|(sr, _)| samples * 1000 / sr as u64).unwrap_or(0);
    (pos, frames, ms)
}

fn id3v1_tail(r: &mut Reader, end: u64) -> u64 {
    match r.bytes::<3>(end) {
        Some(t) if &t == b"TAG" => end + 128,
        _ => end,
    }
}

fn id3_title(tag: &[u8], major: u8) -> Option<String> {
    let mut p = 10usize;
    while p + 10 <= tag.len() {
        let id = &tag[p..p + 4];
        if id[0] == 0 {
            break;
        }
        let raw = crate::util::be32(tag, p + 4)?;
        let size = if major >= 4 {
            ((raw >> 24 & 0x7F) << 21
                | (raw >> 16 & 0x7F) << 14
                | (raw >> 8 & 0x7F) << 7
                | raw & 0x7F) as usize
        } else {
            raw as usize
        };
        if id == b"TIT2" && size > 1 && p + 10 + size <= tag.len() {
            let data = &tag[p + 10..p + 10 + size];
            return Some(match data[0] {
                1 | 2 => {
                    let body = &data[1..];
                    let (le, body) = match body {
                        [0xFF, 0xFE, rest @ ..] => (true, rest),
                        [0xFE, 0xFF, rest @ ..] => (false, rest),
                        _ => (data[0] == 1, body),
                    };
                    let u: Vec<u16> = body
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| {
                            if le {
                                u16::from_le_bytes([c[0], c[1]])
                            } else {
                                u16::from_be_bytes([c[0], c[1]])
                            }
                        })
                        .take_while(|&c| c != 0)
                        .collect();
                    String::from_utf16_lossy(&u)
                }
                _ => String::from_utf8_lossy(&data[1..])
                    .trim_end_matches('\0')
                    .to_string(),
            });
        }
        p += 10 + size;
    }
    None
}

/// MP3 with an ID3v2 tag.
pub fn mp3_id3(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.bytes::<10>(start)?;
    let major = h[3];
    if !(2..=4).contains(&major) || h[4] == 0xFF || h[6..10].iter().any(|&b| b & 0x80 != 0) {
        return None;
    }
    let size = (h[6] as u64) << 21 | (h[7] as u64) << 14 | (h[8] as u64) << 7 | h[9] as u64;
    let footer = if h[5] & 0x10 != 0 { 10 } else { 0 };
    let tag_len = 10 + size + footer;
    let title = if tag_len <= 256 * 1024 {
        r.vec(start, tag_len as usize)
            .and_then(|t| id3_title(&t, major))
    } else {
        None
    };
    let mut audio = start + tag_len;
    // Some encoders pad past the declared size with zeros.
    for _ in 0..4096 {
        match r.u8(audio) {
            Some(0) => audio += 1,
            _ => break,
        }
    }
    let (end, frames, ms) = walk_mpeg(r, audio);
    if frames < 2 {
        return None;
    }
    let end = id3v1_tail(r, end);
    let mut meta = FileMeta::duration(ms).unwrap_or_default();
    meta.title = title.and_then(FileMeta::title).and_then(|m| m.title);
    Some(Carved::new(end - start, Confidence::High).meta(Some(meta)))
}

/// MP3 without a tag: needs a long run of consistent frames.
pub fn mp3_frames(r: &mut Reader, start: u64) -> Option<Carved> {
    let (end, frames, ms) = walk_mpeg(r, start);
    if frames < 8 {
        return None;
    }
    let end = id3v1_tail(r, end);
    Some(Carved::new(end - start, Confidence::Medium).meta(FileMeta::duration(ms)))
}

// ---------------------------------------------------------------- AAC ADTS

pub fn aac_adts(r: &mut Reader, start: u64) -> Option<Carved> {
    const SR: [u32; 13] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ];
    let mut pos = start;
    let mut frames = 0u64;
    let mut first_sf = None;
    while frames < 5_000_000 {
        let Some(h) = r.bytes::<7>(pos) else { break };
        if h[0] != 0xFF || h[1] & 0xF6 != 0xF0 {
            break;
        }
        let sf = (h[2] >> 2) & 0xF;
        if sf as usize >= SR.len() {
            break;
        }
        if *first_sf.get_or_insert(sf) != sf {
            break;
        }
        let len = ((h[3] as u64 & 3) << 11) | ((h[4] as u64) << 3) | (h[5] as u64 >> 5);
        if len < 7 {
            break;
        }
        pos += len;
        frames += 1;
    }
    if frames < 8 {
        return None;
    }
    let sr = SR[first_sf? as usize] as u64;
    Some(
        Carved::new(pos - start, Confidence::Medium)
            .meta(FileMeta::duration(frames * 1024 * 1000 / sr)),
    )
}

// ---------------------------------------------------------------- WAV / AIFF

pub fn wav(r: &mut Reader, start: u64) -> Option<Carved> {
    if r.bytes::<4>(start + 8)? != *b"WAVE" {
        return None;
    }
    let first = r.bytes::<4>(start + 12)?;
    if !matches!(&first, b"fmt " | b"JUNK" | b"bext" | b"LIST" | b"fact") {
        return None;
    }
    let len = r.u32le(start + 4)? as u64 + 8;
    let mut meta = None;
    if &first == b"fmt " {
        let byte_rate = r.u32le(start + 28)? as u64;
        if let Some(ms) = (len.saturating_sub(44) * 1000).checked_div(byte_rate) {
            meta = FileMeta::duration(ms);
        }
    }
    Some(Carved::new(len + (len & 1), Confidence::High).meta(meta))
}

pub fn aiff(r: &mut Reader, start: u64) -> Option<Carved> {
    let form = r.bytes::<4>(start + 8)?;
    if &form != b"AIFF" && &form != b"AIFC" {
        return None;
    }
    let chunk = r.bytes::<4>(start + 12)?;
    if !matches!(&chunk, b"COMM" | b"FVER" | b"NAME" | b"ANNO" | b"(c) ") {
        return None;
    }
    let len = r.u32be(start + 4)? as u64 + 8;
    Some(Carved::new(len + (len & 1), Confidence::High))
}

// ---------------------------------------------------------------- MIDI

pub fn midi(r: &mut Reader, start: u64) -> Option<Carved> {
    if r.u32be(start + 4)? != 6 {
        return None;
    }
    let format = r.u16be(start + 8)?;
    let tracks = r.u16be(start + 10)? as u64;
    if format > 2 || tracks == 0 || tracks > 1024 {
        return None;
    }
    let mut pos = start + 14;
    for _ in 0..tracks {
        if r.bytes::<4>(pos)? != *b"MTrk" {
            return None;
        }
        pos += 8 + r.u32be(pos + 4)? as u64;
    }
    Some(Carved::new(pos - start, Confidence::High))
}

// ---------------------------------------------------------------- OGG

pub fn ogg(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start;
    let mut bos: Vec<u32> = Vec::new();
    let mut eos: Vec<u32> = Vec::new();
    let mut ext = "ogg";
    let mut pages = 0u64;
    while pages < 10_000_000 {
        let Some(h) = r.bytes::<27>(pos) else { break };
        if &h[0..4] != b"OggS" || h[4] != 0 {
            break;
        }
        let ty = h[5];
        let serial = u32::from_le_bytes([h[14], h[15], h[16], h[17]]);
        let nseg = h[26] as usize;
        let Some(segs) = r.vec(pos + 27, nseg) else {
            break;
        };
        let body: u64 = segs.iter().map(|&s| s as u64).sum();
        if pages == 0 {
            if ty & 2 == 0 {
                return None;
            }
            if let Some(p) = r.bytes::<8>(pos + 27 + nseg as u64) {
                if &p == b"OpusHead" {
                    ext = "opus";
                }
            }
        }
        if ty & 2 != 0 && !bos.contains(&serial) {
            bos.push(serial);
        }
        if ty & 4 != 0 && !eos.contains(&serial) {
            eos.push(serial);
        }
        pos += 27 + nseg as u64 + body;
        pages += 1;
        if !bos.is_empty() && bos.iter().all(|s| eos.contains(s)) {
            return Some(Carved::new(pos - start, Confidence::High).ext(ext));
        }
    }
    (pages >= 2).then(|| Carved::new(pos - start, Confidence::Low).ext(ext))
}

// ---------------------------------------------------------------- FLAC

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn crc16_table() -> &'static [u16; 256] {
    static T: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u16; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = (i as u16) << 8;
            for _ in 0..8 {
                c = if c & 0x8000 != 0 {
                    (c << 1) ^ 0x8005
                } else {
                    c << 1
                };
            }
            *e = c;
        }
        t
    })
}

/// Parse a FLAC frame header from a slice → block size (samples). Checks the header CRC-8.
fn flac_frame_header(h: &[u8]) -> Option<u32> {
    if h.len() < 6 || h[0] != 0xFF || h[1] & 0xFE != 0xF8 {
        return None;
    }
    let bs_code = h[2] >> 4;
    let sr_code = h[2] & 0xF;
    if bs_code == 0 || sr_code == 15 || h[3] >> 4 >= 11 || h[3] & 1 != 0 || (h[3] >> 1) & 7 == 7 {
        return None;
    }
    let b = h[4];
    let n = match b {
        0x00..=0x7F => 1,
        _ if b & 0xE0 == 0xC0 => 2,
        _ if b & 0xF0 == 0xE0 => 3,
        _ if b & 0xF8 == 0xF0 => 4,
        _ if b & 0xFC == 0xF8 => 5,
        _ if b & 0xFE == 0xFC => 6,
        0xFE => 7,
        _ => return None,
    };
    let mut p = 4 + n;
    if h.len() < p + 4 || h[5..p].iter().any(|&c| c & 0xC0 != 0x80) {
        return None;
    }
    let bs = match bs_code {
        1 => 192,
        2..=5 => 576u32 << (bs_code - 2),
        6 => {
            p += 1;
            h[p - 1] as u32 + 1
        }
        7 => {
            p += 2;
            u16::from_be_bytes([h[p - 2], h[p - 1]]) as u32 + 1
        }
        _ => 256u32 << (bs_code - 8),
    };
    match sr_code {
        12 => p += 1,
        13 | 14 => p += 2,
        _ => {}
    }
    if h.len() <= p || crc8(&h[..p]) != h[p] {
        return None;
    }
    Some(bs)
}

/// FLAC: metadata blocks, then a frame walk where each frame's end is found by its CRC-16
/// and confirmed by the next frame header.
pub fn flac(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start + 4;
    let mut first = true;
    let (mut min_frame, mut max_frame, mut sr, mut total) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..1024 {
        let h = r.bytes::<4>(pos)?;
        let ty = h[0] & 0x7F;
        let len = u32::from_be_bytes([0, h[1], h[2], h[3]]) as u64;
        if ty == 127 || (first && (ty != 0 || len != 34)) {
            return None;
        }
        if ty == 0 {
            let si = r.bytes::<34>(pos + 4)?;
            min_frame = u32::from_be_bytes([0, si[4], si[5], si[6]]) as u64;
            max_frame = u32::from_be_bytes([0, si[7], si[8], si[9]]) as u64;
            let x = u64::from_be_bytes([
                si[10], si[11], si[12], si[13], si[14], si[15], si[16], si[17],
            ]);
            sr = x >> 44;
            total = x & 0xF_FFFF_FFFF;
        }
        first = false;
        pos += 4 + len;
        if h[0] & 0x80 != 0 {
            break;
        }
    }
    let table = crc16_table();
    let mut samples = 0u64;
    let mut frames = 0u64;
    let window = if max_frame > 0 {
        max_frame + 64
    } else {
        1 << 20
    };
    let min_len = min_frame.max(10);
    let size = r.size();
    while frames < 10_000_000 {
        let avail = (size - pos.min(size)).min(window + 32);
        if avail < 16 {
            break;
        }
        let Some(buf) = r.vec(pos, avail as usize) else {
            break;
        };
        let Some(bs) = flac_frame_header(&buf) else {
            break;
        };
        let mut crc = 0u16;
        let mut end = None;
        for (k, &b) in buf.iter().enumerate() {
            crc = (crc << 8) ^ table[(((crc >> 8) as u8) ^ b) as usize];
            let len = k as u64 + 1;
            if crc != 0 || len < min_len {
                continue;
            }
            let last = total > 0 && samples + bs as u64 >= total;
            let next_ok = flac_frame_header(&buf[k + 1..]).is_some();
            if next_ok || last || pos + len == size {
                end = Some(len);
                break;
            }
        }
        let Some(len) = end else { break };
        pos += len;
        samples += bs as u64;
        frames += 1;
        if total > 0 && samples >= total {
            break;
        }
    }
    if frames == 0 {
        return None;
    }
    let complete = total > 0 && samples >= total;
    let ms = (samples * 1000).checked_div(sr).unwrap_or(0);
    Some(
        Carved::new(
            pos - start,
            if complete {
                Confidence::High
            } else {
                Confidence::Medium
            },
        )
        .meta(FileMeta::duration(ms)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpeg_header() {
        // MPEG-1 Layer III, 128 kbps, 44.1 kHz, no padding → 417 bytes.
        let (len, samples, sr, _) = mpeg_frame([0xFF, 0xFB, 0x90, 0x00]).unwrap();
        assert_eq!((len, samples, sr), (417, 1152, 44_100));
        assert!(mpeg_frame([0xFF, 0xFB, 0xF0, 0x00]).is_none());
    }

    #[test]
    fn crc8_known() {
        assert_eq!(crc8(b"123456789"), 0xF4);
    }
}
