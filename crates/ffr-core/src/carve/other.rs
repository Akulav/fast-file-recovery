//! Other formats: SQLite, PE, ELF, Mach-O, PST/OST, KDBX, VMDK/VHD/VHDX, fonts.

use super::{is_fourcc, Carved, Confidence, Reader};

pub fn sqlite(r: &mut Reader, start: u64) -> Option<Carved> {
    let raw = r.u16be(start + 16)? as u64;
    let page = if raw == 1 { 65_536 } else { raw };
    if page < 512 || !page.is_power_of_two() {
        return None;
    }
    let h = r.bytes::<4>(start + 20)?; // reserved, max/min/leaf payload fractions
    if h[1] != 64 || h[2] != 32 || h[3] != 32 {
        return None;
    }
    let pages = r.u32be(start + 28)? as u64;
    let change = r.u32be(start + 24)?;
    let valid_for = r.u32be(start + 92)?;
    if pages == 0 || change != valid_for {
        return Some(Carved::new(page, Confidence::Low));
    }
    Some(Carved::new(page * pages, Confidence::High))
}

/// PE (EXE/DLL): furthest section end, including the certificate table.
pub fn pe(r: &mut Reader, start: u64) -> Option<Carved> {
    let lfanew = r.u32le(start + 0x3C)? as u64;
    if !(0x40..=0x1000).contains(&lfanew) || r.bytes::<4>(start + lfanew)? != *b"PE\0\0" {
        return None;
    }
    let coff = start + lfanew + 4;
    let nsect = r.u16le(coff + 2)? as u64;
    let opt_size = r.u16le(coff + 16)? as u64;
    let chars = r.u16le(coff + 18)?;
    if nsect == 0 || nsect > 96 {
        return None;
    }
    let opt = coff + 20;
    let magic = r.u16le(opt)?;
    let dirs = match magic {
        0x10B => opt + 96,
        0x20B => opt + 112,
        _ => return None,
    };
    let mut end = lfanew + 24 + opt_size + nsect * 40;
    let sects = opt + opt_size;
    for i in 0..nsect {
        let s = sects + i * 40;
        let raw_size = r.u32le(s + 16)? as u64;
        let raw_ptr = r.u32le(s + 20)? as u64;
        if raw_size > 0 {
            end = end.max(raw_ptr + raw_size);
        }
    }
    // Certificate table (data directory 4) uses file offsets.
    if dirs + 40 <= opt + opt_size {
        let cert_off = r.u32le(dirs + 32)? as u64;
        let cert_len = r.u32le(dirs + 36)? as u64;
        if cert_off > 0 && cert_len > 0 && cert_len < 64 << 20 {
            end = end.max(cert_off + cert_len);
        }
    }
    let ext = if chars & 0x2000 != 0 { "dll" } else { "exe" };
    Some(Carved::new(end, Confidence::Medium).ext(ext))
}

pub fn elf(r: &mut Reader, start: u64) -> Option<Carved> {
    let id = r.bytes::<16>(start)?;
    let class = id[4];
    let le = match id[5] {
        1 => true,
        2 => false,
        _ => return None,
    };
    if !matches!(class, 1 | 2) || id[6] != 1 {
        return None;
    }
    let u16_ = |r: &mut Reader, o| if le { r.u16le(o) } else { r.u16be(o) };
    let u32_ = |r: &mut Reader, o| if le { r.u32le(o) } else { r.u32be(o) };
    let u64_ = |r: &mut Reader, o| if le { r.u64le(o) } else { r.u64be(o) };
    let word = |r: &mut Reader, o| {
        if class == 2 {
            u64_(r, o)
        } else {
            u32_(r, o).map(u64::from)
        }
    };
    let (phoff, shoff, phentsize, phnum, shentsize, shnum) = if class == 2 {
        (
            u64_(r, start + 32)?,
            u64_(r, start + 40)?,
            u16_(r, start + 54)? as u64,
            u16_(r, start + 56)? as u64,
            u16_(r, start + 58)? as u64,
            u16_(r, start + 60)? as u64,
        )
    } else {
        (
            u32_(r, start + 28)? as u64,
            u32_(r, start + 32)? as u64,
            u16_(r, start + 42)? as u64,
            u16_(r, start + 44)? as u64,
            u16_(r, start + 46)? as u64,
            u16_(r, start + 48)? as u64,
        )
    };
    if phnum > 4096 || shnum > 65_535 {
        return None;
    }
    let mut end = shoff + shnum * shentsize;
    end = end.max(phoff + phnum * phentsize);
    for i in 0..phnum {
        let p = start + phoff + i * phentsize;
        let (off, filesz) = if class == 2 {
            (word(r, p + 8)?, word(r, p + 32)?)
        } else {
            (word(r, p + 4)?, word(r, p + 16)?)
        };
        end = end.max(off + filesz);
    }
    for i in 0..shnum.min(4096) {
        let s = start + shoff + i * shentsize;
        let ty = u32_(r, s + 4)?;
        if ty == 8 {
            continue; // NOBITS
        }
        let (off, size) = if class == 2 {
            (word(r, s + 24)?, word(r, s + 32)?)
        } else {
            (word(r, s + 16)?, word(r, s + 20)?)
        };
        end = end.max(off + size);
    }
    (end > 52).then(|| Carved::new(end, Confidence::Medium))
}

pub fn macho(r: &mut Reader, start: u64) -> Option<Carved> {
    let magic = r.u32be(start)?;
    if magic == 0xCAFE_BABE {
        let n = r.u32be(start + 4)? as u64;
        if n == 0 || n > 16 {
            return None; // Java class files share this magic but have a large "count".
        }
        let mut end = 8 + n * 20;
        for i in 0..n {
            let a = start + 8 + i * 20;
            let off = r.u32be(a + 8)? as u64;
            let size = r.u32be(a + 12)? as u64;
            if off < end {
                return None;
            }
            end = end.max(off + size);
        }
        return Some(Carved::new(end, Confidence::Medium));
    }
    let (le, is64) = match magic {
        0xFEED_FACE => (false, false),
        0xFEED_FACF => (false, true),
        0xCEFA_EDFE => (true, false),
        0xCFFA_EDFE => (true, true),
        _ => return None,
    };
    let u32_ = |r: &mut Reader, o| if le { r.u32le(o) } else { r.u32be(o) };
    let u64_ = |r: &mut Reader, o| if le { r.u64le(o) } else { r.u64be(o) };
    let ncmds = u32_(r, start + 16)? as u64;
    let sizeofcmds = u32_(r, start + 20)? as u64;
    if ncmds == 0 || ncmds > 4096 || sizeofcmds > 16 << 20 {
        return None;
    }
    let hdr = if is64 { 32 } else { 28 };
    let mut end = hdr + sizeofcmds;
    let mut p = start + hdr;
    for _ in 0..ncmds {
        let cmd = u32_(r, p)?;
        let size = u32_(r, p + 4)? as u64;
        if size < 8 {
            return None;
        }
        match cmd {
            0x1 => {
                let off = u32_(r, p + 32)? as u64;
                let sz = u32_(r, p + 36)? as u64;
                end = end.max(off + sz);
            }
            0x19 => {
                let off = u64_(r, p + 40)?;
                let sz = u64_(r, p + 48)?;
                end = end.max(off + sz);
            }
            0x1D => {
                let off = u32_(r, p + 8)? as u64;
                let sz = u32_(r, p + 12)? as u64;
                end = end.max(off + sz);
            }
            _ => {}
        }
        p += size;
    }
    Some(Carved::new(end, Confidence::Medium))
}

pub fn pst(r: &mut Reader, start: u64) -> Option<Carved> {
    let client = r.bytes::<2>(start + 8)?;
    let ext = match &client {
        b"SM" => "pst",
        b"SO" => "ost",
        _ => return None,
    };
    let ver = r.u16le(start + 10)?;
    let len = match ver {
        14 | 15 => r.u32le(start + 0xA8)? as u64,
        23 | 36 => r.u64le(start + 0xB8)?,
        _ => return None,
    };
    Some(Carved::new(len, Confidence::High).ext(ext))
}

/// KeePass KDBX 4: header fields, then HMAC block stream ending with an empty block.
pub fn kdbx(r: &mut Reader, start: u64) -> Option<Carved> {
    let major = r.u16le(start + 10)?;
    if major != 4 {
        return Some(Carved::new(64 * 1024, Confidence::Low));
    }
    let mut pos = start + 12;
    for _ in 0..64 {
        let id = r.u8(pos)?;
        let len = r.u32le(pos + 1)? as u64;
        if len > 1 << 20 {
            return None;
        }
        pos += 5 + len;
        if id == 0 {
            break;
        }
    }
    pos += 64; // header SHA-256 + HMAC
    for _ in 0..1_000_000 {
        let size = r.u32le(pos + 32)? as u64;
        pos += 36 + size;
        if size == 0 {
            return Some(Carved::new(pos - start, Confidence::High));
        }
    }
    None
}

pub fn vhd(r: &mut Reader, start: u64) -> Option<Carved> {
    let disk_type = r.u32be(start + 60)?;
    if disk_type != 3 && disk_type != 4 {
        return None; // fixed disks only have a footer at the end
    }
    let dyn_off = r.u64be(start + 16)?;
    if r.bytes::<8>(start + dyn_off)? != *b"cxsparse" {
        return None;
    }
    let bat = r.u64be(start + dyn_off + 16)?;
    let entries = r.u32be(start + dyn_off + 28)? as u64;
    let block = r.u32be(start + dyn_off + 32)? as u64;
    if entries > 1 << 22 || block == 0 {
        return None;
    }
    let table = r.vec(start + bat, (entries * 4) as usize)?;
    let bitmap = (block / 512).div_ceil(8).div_ceil(512) * 512;
    let mut end = bat + entries * 4;
    for c in table.as_chunks::<4>().0 {
        let s = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        if s != 0xFFFF_FFFF {
            end = end.max(s as u64 * 512 + bitmap + block);
        }
    }
    Some(Carved::new(
        crate::util::align_up(end, 512) + 512,
        Confidence::Medium,
    ))
}

/// TrueType / OpenType: table directory → furthest table end.
pub fn sfnt(r: &mut Reader, start: u64) -> Option<Carved> {
    let n = r.u16be(start + 4)? as u64;
    if n == 0 || n > 64 {
        return None;
    }
    let search = r.u16be(start + 6)? as u64;
    let pow = 1u64 << (63 - n.leading_zeros() as u64);
    if search != pow * 16 {
        return None;
    }
    let mut end = 12 + n * 16;
    let ext = if r.bytes::<4>(start)? == *b"OTTO" {
        "otf"
    } else {
        "ttf"
    };
    for i in 0..n {
        let rec = r.bytes::<16>(start + 12 + i * 16)?;
        if !is_fourcc(&rec[0..4]) {
            return None;
        }
        let off = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as u64;
        let len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as u64;
        if off < 12 + n * 16 {
            return None;
        }
        end = end.max(crate::util::align_up(off + len, 4));
    }
    Some(Carved::new(end, Confidence::High).ext(ext))
}

pub fn woff(r: &mut Reader, start: u64) -> Option<Carved> {
    let flavor = r.bytes::<4>(start + 4)?;
    if flavor != [0, 1, 0, 0] && &flavor != b"OTTO" && &flavor != b"true" {
        return None;
    }
    let len = r.u32be(start + 8)? as u64;
    let ext = if r.bytes::<4>(start)? == *b"wOF2" {
        "woff2"
    } else {
        "woff"
    };
    Some(Carved::new(len, Confidence::High).ext(ext))
}
