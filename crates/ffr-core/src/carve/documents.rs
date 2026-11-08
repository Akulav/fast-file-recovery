//! Documents: PDF, ZIP-based formats (DOCX/XLSX/PPTX/ODF/EPUB/JAR/APK), OLE compound files
//! (DOC/XLS/PPT/MSG), RTF and plain-text markup.

use super::{text_run, Carved, Confidence, FileMeta, Reader};

// ---------------------------------------------------------------- PDF

/// PDF: use `/Linearized /L <len>` when present; otherwise the last `%%EOF` before another
/// `%PDF-` header, giving up 8 MiB after the last `%%EOF` seen.
pub fn pdf(r: &mut Reader, start: u64) -> Option<Carved> {
    let v = r.bytes::<8>(start)?;
    if !v[5].is_ascii_digit() || v[6] != b'.' || !v[7].is_ascii_digit() {
        return None;
    }
    let head_len = (r.size() - start).min(1024) as usize;
    let head = r.vec(start, head_len)?;
    let mut title = None;
    if let Some(i) = memchr::memmem::find(&head, b"/Linearized") {
        if let Some(j) = memchr::memmem::find(&head[i..], b"/L ") {
            let digits: String = head[i + j + 3..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .map(|&c| c as char)
                .collect();
            if let Ok(len) = digits.parse::<u64>() {
                if len > 64 {
                    if let Some(tail) = r.vec(start + len - 64.min(len), 64.min(len) as usize) {
                        if memchr::memmem::find(&tail, b"%%EOF").is_some() {
                            return Some(Carved::new(len, Confidence::High));
                        }
                    }
                }
            }
        }
    }
    if let Some(i) = memchr::memmem::find(&head, b"/Title") {
        let rest = &head[i + 6..];
        if let Some(open) = rest.iter().position(|&c| c == b'(') {
            if let Some(close) = rest[open..].iter().position(|&c| c == b')') {
                title = Some(String::from_utf8_lossy(&rest[open + 1..open + close]).into_owned());
            }
        }
    }
    let eof = memchr::memmem::Finder::new(b"%%EOF");
    let hdr = memchr::memmem::Finder::new(b"%PDF-");
    let mut last_eof: Option<u64> = None;
    r.scan(start + 8, 1 << 30, 5, |at, piece| {
        let stop = hdr.find(piece);
        let region = &piece[..stop.unwrap_or(piece.len())];
        for i in eof.find_iter(region) {
            last_eof = Some(at + i as u64);
        }
        if stop.is_some() {
            return false;
        }
        match last_eof {
            Some(e) => at + piece.len() as u64 <= e + (8 << 20),
            None => true,
        }
    });
    let e = last_eof? + 5;
    let mut end = e;
    if let Some(t) = r.bytes::<2>(e) {
        if t == *b"\r\n" {
            end += 2;
        } else if t[0] == b'\n' || t[0] == b'\r' {
            end += 1;
        }
    }
    Some(Carved::new(end - start, Confidence::Medium).meta(title.and_then(FileMeta::title)))
}

// ---------------------------------------------------------------- ZIP

const LOCAL: u32 = 0x0403_4B50;
const CENTRAL: u32 = 0x0201_4B50;
const EOCD: u32 = 0x0605_4B50;
const EOCD64: u32 = 0x0606_4B50;
const LOC64: u32 = 0x0706_4B50;
const DESCRIPTOR: u32 = 0x0807_4B50;
const DIGSIG: u32 = 0x0505_4B50;

fn next_pk(r: &mut Reader, mut from: u64, limit: u64) -> Option<u64> {
    let end = from.saturating_add(limit);
    while from < end {
        let p = r.find(from, end - from, b"PK")?;
        if let Some(sig) = r.u32le(p) {
            if matches!(sig, LOCAL | CENTRAL | DESCRIPTOR | EOCD | EOCD64) {
                return Some(p);
            }
        }
        from = p + 1;
    }
    None
}

fn zip_subtype(names: &[String], mimetype: &str) -> &'static str {
    match mimetype {
        "application/epub+zip" => return "epub",
        "application/vnd.oasis.opendocument.text" => return "odt",
        "application/vnd.oasis.opendocument.spreadsheet" => return "ods",
        "application/vnd.oasis.opendocument.presentation" => return "odp",
        _ => {}
    }
    let any = |p: &str| names.iter().any(|n| n.starts_with(p));
    if any("[Content_Types].xml") || any("_rels/") {
        if any("word/") {
            return "docx";
        }
        if any("xl/") {
            return "xlsx";
        }
        if any("ppt/") {
            return "pptx";
        }
    }
    if any("AndroidManifest.xml") || any("classes.dex") {
        return "apk";
    }
    if any("META-INF/MANIFEST.MF") {
        return "jar";
    }
    "zip"
}

/// ZIP and everything built on it: local headers → central directory → EOCD.
pub fn zip(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut pos = start;
    let mut names: Vec<String> = Vec::new();
    let mut mimetype = String::new();
    let mut entries = 0u64;
    for _ in 0..2_000_000 {
        let Some(sig) = r.u32le(pos) else { break };
        match sig {
            LOCAL => {
                let h = r.bytes::<30>(pos)?;
                let version = u16::from_le_bytes([h[4], h[5]]);
                let flags = u16::from_le_bytes([h[6], h[7]]);
                let method = u16::from_le_bytes([h[8], h[9]]);
                let mut csize = u32::from_le_bytes([h[18], h[19], h[20], h[21]]) as u64;
                let usize_ = u32::from_le_bytes([h[22], h[23], h[24], h[25]]) as u64;
                let nlen = u16::from_le_bytes([h[26], h[27]]) as u64;
                let elen = u16::from_le_bytes([h[28], h[29]]) as u64;
                if (version & 0xFF) > 90
                    || !matches!(
                        method,
                        0 | 1 | 6 | 8 | 9 | 12 | 14 | 19 | 93 | 95 | 96 | 97 | 98 | 99
                    )
                    || nlen == 0
                {
                    if entries == 0 {
                        return None;
                    }
                    break;
                }
                let name = r.vec(pos + 30, nlen as usize)?;
                if entries == 0 && name.iter().any(|&c| c < 0x20) {
                    return None;
                }
                let name = String::from_utf8_lossy(&name).into_owned();
                if csize == 0xFFFF_FFFF && elen >= 20 {
                    if let Some(extra) = r.vec(pos + 30 + nlen, elen as usize) {
                        let mut p = 0;
                        while p + 4 <= extra.len() {
                            let id = u16::from_le_bytes([extra[p], extra[p + 1]]);
                            let sz = u16::from_le_bytes([extra[p + 2], extra[p + 3]]) as usize;
                            if id == 1 {
                                if let Some(c) = crate::util::le64(&extra, p + 12) {
                                    csize = c;
                                }
                                break;
                            }
                            p += 4 + sz;
                        }
                    }
                }
                let data = pos + 30 + nlen + elen;
                if entries == 0 && name == "mimetype" && method == 0 && usize_ < 128 {
                    if let Some(m) = r.vec(data, usize_ as usize) {
                        mimetype = String::from_utf8_lossy(&m).trim().to_string();
                    }
                }
                if names.len() < 64 {
                    names.push(name);
                }
                entries += 1;
                if flags & 8 != 0 && csize == 0 {
                    let next = next_pk(r, data, 1 << 32)?;
                    pos = if r.u32le(next) == Some(DESCRIPTOR) {
                        // 16 bytes, or 24 with ZIP64 sizes; pick the one followed by a header.
                        if matches!(r.u32le(next + 16), Some(LOCAL | CENTRAL)) {
                            next + 16
                        } else {
                            next + 24
                        }
                    } else {
                        next
                    };
                } else {
                    pos = data + csize;
                    if flags & 8 != 0 {
                        if r.u32le(pos) == Some(DESCRIPTOR) {
                            pos += 16;
                        } else if matches!(r.u32le(pos + 12), Some(LOCAL | CENTRAL)) {
                            pos += 12;
                        }
                    }
                }
            }
            CENTRAL => {
                let h = r.bytes::<46>(pos)?;
                let nlen = u16::from_le_bytes([h[28], h[29]]) as u64;
                let elen = u16::from_le_bytes([h[30], h[31]]) as u64;
                let clen = u16::from_le_bytes([h[32], h[33]]) as u64;
                pos += 46 + nlen + elen + clen;
            }
            EOCD => {
                let clen = r.u16le(pos + 20)? as u64;
                let end = pos + 22 + clen;
                if entries == 0 {
                    return None;
                }
                return Some(
                    Carved::new(end - start, Confidence::High).ext(zip_subtype(&names, &mimetype)),
                );
            }
            EOCD64 => pos += 12 + r.u64le(pos + 4)?,
            LOC64 => pos += 20,
            DIGSIG => pos += 6 + r.u16le(pos + 4)? as u64,
            _ => break,
        }
    }
    if entries == 0 {
        return None;
    }
    Some(Carved::new(pos - start, Confidence::Low).ext(zip_subtype(&names, &mimetype)))
}

// ---------------------------------------------------------------- OLE compound file

const FREESECT: u32 = 0xFFFF_FFFF;
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;

/// OLE CFB: size = (highest used sector + 1) × sector size + header.
pub fn ole(r: &mut Reader, start: u64) -> Option<Carved> {
    let h = r.vec(start, 512)?;
    let le16 = |o| crate::util::le16(&h, o);
    let le32 = |o| crate::util::le32(&h, o);
    let major = le16(26)?;
    let shift = le16(30)?;
    if le16(28)? != 0xFFFE || !matches!((major, shift), (3, 9) | (4, 12)) || le16(32)? != 6 {
        return None;
    }
    let ss = 1u64 << shift;
    let num_fat = le32(44)? as u64;
    let first_dir = le32(48)?;
    let mut difat_next = le32(68)?;
    let num_difat = le32(72)?;
    if num_fat == 0 || num_fat > 1 << 20 || num_difat > 1 << 16 {
        return None;
    }
    let sector_off = |s: u32| start + (s as u64 + 1) * ss;
    let mut fat_sectors: Vec<u32> = (0..109)
        .filter_map(|i| le32(76 + i * 4))
        .filter(|&s| s != FREESECT)
        .collect();
    for _ in 0..num_difat {
        if difat_next == ENDOFCHAIN || difat_next == FREESECT {
            break;
        }
        let d = r.vec(sector_off(difat_next), ss as usize)?;
        let per = (ss / 4 - 1) as usize;
        for i in 0..per {
            let s = crate::util::le32(&d, i * 4)?;
            if s != FREESECT {
                fat_sectors.push(s);
            }
        }
        difat_next = crate::util::le32(&d, per * 4)?;
    }
    fat_sectors.truncate(num_fat as usize);
    if (fat_sectors.len() as u64) < num_fat {
        return None;
    }
    let mut fat: Vec<u32> = Vec::with_capacity(fat_sectors.len() * (ss as usize / 4));
    for &s in &fat_sectors {
        let d = r.vec(sector_off(s), ss as usize)?;
        fat.extend(
            d.as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        );
    }
    let max_used = fat.iter().rposition(|&e| e != FREESECT)? as u64;
    let len = (max_used + 2) * ss;
    // Directory names → sub-type.
    let mut ext = "ole";
    let mut s = first_dir;
    for _ in 0..16 {
        if s as usize >= fat.len() {
            break;
        }
        let Some(d) = r.vec(sector_off(s), ss as usize) else {
            break;
        };
        for e in d.as_chunks::<128>().0 {
            let nlen = (u16::from_le_bytes([e[64], e[65]]) as usize).min(64);
            let name = crate::util::utf16le(&e[..nlen]);
            ext = match name.as_str() {
                "WordDocument" => "doc",
                "Workbook" | "Book" => "xls",
                "PowerPoint Document" => "ppt",
                n if n.starts_with("__substg1.0_") || n == "__properties_version1.0" => "msg",
                _ => continue,
            };
            break;
        }
        if ext != "ole" {
            break;
        }
        s = fat[s as usize];
        if s == ENDOFCHAIN {
            break;
        }
    }
    Some(Carved::new(len, Confidence::High).ext(ext))
}

// ---------------------------------------------------------------- RTF / text markup

/// RTF: balance braces (ignoring escaped ones) until the outer group closes.
pub fn rtf(r: &mut Reader, start: u64) -> Option<Carved> {
    let mut depth = 0i64;
    let mut end = None;
    let mut escape = false;
    let mut seen = 0u64;
    r.scan(start, 256 << 20, 0, |at, piece| {
        for (i, &c) in piece.iter().enumerate() {
            seen += 1;
            if escape {
                escape = false;
                continue;
            }
            match c {
                b'\\' => escape = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + i as u64 + 1);
                        return false;
                    }
                }
                0 => return false,
                _ => {}
            }
        }
        true
    });
    let _ = seen;
    end.map(|e| Carved::new(e - start, Confidence::Medium))
}

/// XML / HTML / e-mail: a run of text bytes.
pub fn text(r: &mut Reader, start: u64) -> Option<Carved> {
    let len = text_run(r, start, 64 << 20);
    (len >= 32).then(|| Carved::new(len, Confidence::Low))
}
