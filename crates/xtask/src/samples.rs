//! Generates the small sample files used by the engine tests (`crates/ffr-core/tests/samples`).
//!
//! Every file is created here from scratch (images through the `image` encoders, the rest
//! built byte by byte), so they are freely licensed and each stays under 50 KiB.
//! File names are `<signature id>.<expected extension>`.

use crate::Result;
use image::{DynamicImage, ImageFormat, RgbImage};
use std::io::Cursor;
use std::path::Path;

// ---------------------------------------------------------------- helpers

fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *e = c;
    }
    t
}

fn crc32(data: &[u8]) -> u32 {
    let t = crc32_table();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

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

fn crc16_flac(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

trait Put {
    fn u16le(&mut self, v: u16);
    fn u32le(&mut self, v: u32);
    fn u64le(&mut self, v: u64);
    fn u16be(&mut self, v: u16);
    fn u32be(&mut self, v: u32);
    fn u64be(&mut self, v: u64);
}

impl Put for Vec<u8> {
    fn u16le(&mut self, v: u16) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u32le(&mut self, v: u32) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u64le(&mut self, v: u64) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u16be(&mut self, v: u16) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn u32be(&mut self, v: u32) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn u64be(&mut self, v: u64) {
        self.extend_from_slice(&v.to_be_bytes());
    }
}

fn set32le(v: &mut [u8], at: usize, x: u32) {
    v[at..at + 4].copy_from_slice(&x.to_le_bytes());
}

fn set32be(v: &mut [u8], at: usize, x: u32) {
    v[at..at + 4].copy_from_slice(&x.to_be_bytes());
}

fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn picture(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) * 3 % 255) as u8,
        ])
    })
}

fn encode(img: DynamicImage, fmt: ImageFormat) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    img.write_to(&mut Cursor::new(&mut v), fmt)
        .map_err(|e| format!("{fmt:?}: {e}"))?;
    Ok(v)
}

// ---------------------------------------------------------------- containers

/// A stored (uncompressed) ZIP.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let off = out.len() as u32;
        let crc = crc32(data);
        out.u32le(0x0403_4B50);
        out.u16le(20);
        out.u16le(0);
        out.u16le(0);
        out.u16le(0);
        out.u16le(0x21);
        out.u32le(crc);
        out.u32le(data.len() as u32);
        out.u32le(data.len() as u32);
        out.u16le(name.len() as u16);
        out.u16le(0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.u32le(0x0201_4B50);
        central.u16le(20);
        central.u16le(20);
        central.u16le(0);
        central.u16le(0);
        central.u16le(0);
        central.u16le(0x21);
        central.u32le(crc);
        central.u32le(data.len() as u32);
        central.u32le(data.len() as u32);
        central.u16le(name.len() as u16);
        central.extend_from_slice(&[0; 12]);
        central.u32le(off);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.u32le(0x0605_4B50);
    out.u32le(0);
    out.u16le(entries.len() as u16);
    out.u16le(entries.len() as u16);
    out.u32le(central.len() as u32);
    out.u32le(cd_off);
    out.u16le(0);
    out
}

fn bmff_box(ty: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.u32be(8 + payload.len() as u32);
    v.extend_from_slice(ty);
    v.extend_from_slice(payload);
    v
}

fn ftyp(major: &[u8; 4], compat: &[&[u8; 4]]) -> Vec<u8> {
    let mut p = major.to_vec();
    p.u32be(0x200);
    for c in compat {
        p.extend_from_slice(*c);
    }
    bmff_box(b"ftyp", &p)
}

fn mvhd(duration_ms: u32) -> Vec<u8> {
    let mut p = vec![0u8; 4]; // version + flags
    p.u32be(0);
    p.u32be(0);
    p.u32be(1000);
    p.u32be(duration_ms);
    p.extend_from_slice(&[0; 80]);
    bmff_box(b"mvhd", &p)
}

fn ebml(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    let n = payload.len();
    if n < 127 {
        v.push(0x80 | n as u8);
    } else {
        v.push(0x40 | (n >> 8) as u8);
        v.push(n as u8);
    }
    v.extend_from_slice(payload);
    v
}

fn ogg_page(ty: u8, seq: u32, granule: u64, packet: &[u8]) -> Vec<u8> {
    let mut p = b"OggS".to_vec();
    p.push(0);
    p.push(ty);
    p.u64le(granule);
    p.u32le(0x1234_5678);
    p.u32le(seq);
    p.u32le(0);
    let mut segs = vec![255u8; packet.len() / 255];
    segs.push((packet.len() % 255) as u8);
    p.push(segs.len() as u8);
    p.extend_from_slice(&segs);
    p.extend_from_slice(packet);
    let c = ogg_crc(&p);
    p[22..26].copy_from_slice(&c.to_le_bytes());
    p
}

fn tar_header(name: &str, size: usize) -> Vec<u8> {
    let mut h = vec![0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0001750\0");
    h[116..124].copy_from_slice(b"0001750\0");
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[136..148].copy_from_slice(b"14620154060\0");
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| b as u32).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    h
}

fn vint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

fn rar5_header(fields: &[u8]) -> Vec<u8> {
    let mut h = vint(fields.len() as u64);
    h.extend_from_slice(fields);
    let mut out = Vec::new();
    out.u32le(crc32(&h));
    out.extend_from_slice(&h);
    out
}

// ---------------------------------------------------------------- the samples

pub fn generate(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut files: Vec<(&str, Vec<u8>)> = Vec::new();
    let pic = DynamicImage::ImageRgb8(picture(96, 64));

    files.push(("jpeg.jpg", encode(pic.clone(), ImageFormat::Jpeg)?));
    files.push(("png.png", encode(pic.clone(), ImageFormat::Png)?));
    files.push((
        "gif.gif",
        encode(DynamicImage::ImageRgba8(pic.to_rgba8()), ImageFormat::Gif)?,
    ));
    files.push(("bmp.bmp", encode(pic.clone(), ImageFormat::Bmp)?));
    files.push(("tiff.tif", encode(pic.clone(), ImageFormat::Tiff)?));
    files.push((
        "webp.webp",
        encode(DynamicImage::ImageRgba8(pic.to_rgba8()), ImageFormat::WebP)?,
    ));
    // ICO with one embedded 16×16 PNG.
    {
        let png = encode(DynamicImage::ImageRgb8(picture(16, 16)), ImageFormat::Png)?;
        let mut v = Vec::new();
        v.u16le(0);
        v.u16le(1);
        v.u16le(1);
        v.extend_from_slice(&[16, 16, 0, 0]);
        v.u16le(1);
        v.u16le(32);
        v.u32le(png.len() as u32);
        v.u32le(22);
        v.extend_from_slice(&png);
        files.push(("ico.ico", v));
    }
    // PSD: 8×8 grayscale, raw.
    {
        let mut v = b"8BPS".to_vec();
        v.u16be(1);
        v.extend_from_slice(&[0; 6]);
        v.u16be(1);
        v.u32be(8);
        v.u32be(8);
        v.u16be(8);
        v.u16be(1);
        v.u32be(0);
        v.u32be(0);
        v.u32be(0);
        v.u16be(0);
        v.extend_from_slice(&pattern(64, 9));
        files.push(("psd.psd", v));
    }
    // HEIC (ISO-BMFF with meta + mdat).
    {
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"pict");
        hdlr.extend_from_slice(&[0; 13]);
        let mut meta = vec![0u8; 4];
        meta.extend_from_slice(&bmff_box(b"hdlr", &hdlr));
        let mut v = ftyp(b"heic", &[b"mif1", b"heic"]);
        v.extend_from_slice(&bmff_box(b"meta", &meta));
        v.extend_from_slice(&bmff_box(b"mdat", &pattern(400, 3)));
        files.push(("heic.heic", v));
    }

    // Video
    {
        let mut v = ftyp(b"isom", &[b"isom", b"iso2", b"mp41"]);
        v.extend_from_slice(&bmff_box(b"moov", &mvhd(2000)));
        v.extend_from_slice(&bmff_box(b"mdat", &pattern(2000, 5)));
        files.push(("mp4.mp4", v));
    }
    {
        let mut v = ftyp(b"qt  ", &[b"qt  "]);
        v.extend_from_slice(&bmff_box(b"wide", &[]));
        v.extend_from_slice(&bmff_box(b"mdat", &pattern(1500, 6)));
        v.extend_from_slice(&bmff_box(b"moov", &mvhd(1500)));
        files.push(("mp4.mov", v));
    }
    {
        let mut avih = Vec::new();
        avih.extend_from_slice(b"avih");
        avih.u32le(56);
        avih.extend_from_slice(&[0; 56]);
        let mut hdrl = b"hdrl".to_vec();
        hdrl.extend_from_slice(&avih);
        let mut movi = b"movi".to_vec();
        movi.extend_from_slice(b"00dc");
        movi.u32le(1024);
        movi.extend_from_slice(&pattern(1024, 7));
        let mut body = b"AVI ".to_vec();
        body.extend_from_slice(b"LIST");
        body.u32le(hdrl.len() as u32);
        body.extend_from_slice(&hdrl);
        body.extend_from_slice(b"LIST");
        body.u32le(movi.len() as u32);
        body.extend_from_slice(&movi);
        body.extend_from_slice(b"idx1");
        body.u32le(16);
        body.extend_from_slice(&[0; 16]);
        let mut v = b"RIFF".to_vec();
        v.u32le(body.len() as u32);
        v.extend_from_slice(&body);
        files.push(("avi.avi", v));
    }
    {
        let mut header = Vec::new();
        header.extend(ebml(&[0x42, 0x86], &[1]));
        header.extend(ebml(&[0x42, 0xF7], &[1]));
        header.extend(ebml(&[0x42, 0xF2], &[4]));
        header.extend(ebml(&[0x42, 0xF3], &[8]));
        header.extend(ebml(&[0x42, 0x82], b"matroska"));
        header.extend(ebml(&[0x42, 0x87], &[4]));
        header.extend(ebml(&[0x42, 0x85], &[2]));
        let mut info = ebml(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]);
        info.extend(ebml(&[0x4D, 0x80], b"ffr"));
        let mut track = ebml(&[0xD7], &[1]);
        track.extend(ebml(&[0x83], &[1]));
        track.extend(ebml(&[0x86], b"V_UNCOMPRESSED"));
        let tracks = ebml(&[0xAE], &track);
        let mut block = vec![0x81, 0, 0, 0x80];
        block.extend_from_slice(&pattern(1200, 8));
        let mut cluster = ebml(&[0xE7], &[0]);
        cluster.extend(ebml(&[0xA3], &block));
        let mut seg = ebml(&[0x15, 0x49, 0xA9, 0x66], &info);
        seg.extend(ebml(&[0x16, 0x54, 0xAE, 0x6B], &tracks));
        seg.extend(ebml(&[0x1F, 0x43, 0xB6, 0x75], &cluster));
        let mut v = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &header);
        v.extend(ebml(&[0x18, 0x53, 0x80, 0x67], &seg));
        files.push(("mkv.mkv", v));
    }
    {
        let mut v = b"FLV".to_vec();
        v.push(1);
        v.push(5);
        v.u32be(9);
        v.u32be(0);
        for (i, (ty, n)) in [(18u8, 40usize), (9, 600), (8, 500)].iter().enumerate() {
            v.push(*ty);
            v.extend_from_slice(&(*n as u32).to_be_bytes()[1..]);
            v.extend_from_slice(&((i as u32) * 40).to_be_bytes()[1..]);
            v.push(0);
            v.extend_from_slice(&[0, 0, 0]);
            v.extend_from_slice(&pattern(*n, *ty));
            v.u32be(11 + *n as u32);
        }
        files.push(("flv.flv", v));
    }
    {
        let mut v = Vec::new();
        for i in 0..16u8 {
            let mut p = vec![0xFFu8; 188];
            p[0] = 0x47;
            if i == 0 {
                p[1] = 0x40;
                p[2] = 0x00;
            } else {
                p[1] = 0x01;
                p[2] = 0x00;
            }
            p[3] = 0x10 | (i & 0x0F);
            v.extend_from_slice(&p);
        }
        files.push(("ts.ts", v));
    }
    {
        // ASF / WMV: header object (file properties) + data object.
        let header_guid = [
            0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
            0xCE, 0x6C,
        ];
        let fprops = [
            0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20,
            0x53, 0x65,
        ];
        let video = [
            0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C,
            0x44, 0x2B,
        ];
        let data_guid = [
            0x36, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
            0xCE, 0x6C,
        ];
        let packets = pattern(1024, 4);
        let data_len = 50 + packets.len() as u64;
        // stream properties stand-in carrying the video media GUID
        let mut stream = vec![0u8; 16];
        stream.u64le(24 + 16);
        stream.extend_from_slice(&video);
        let header_len = 30 + 104 + stream.len() as u64;
        let total = header_len + data_len;
        let mut v = header_guid.to_vec();
        v.u64le(header_len);
        v.u32le(2);
        v.extend_from_slice(&[1, 2]);
        v.extend_from_slice(&fprops);
        v.u64le(104);
        v.extend_from_slice(&[7; 16]);
        v.u64le(total);
        v.u64le(0);
        v.u64le(4);
        v.u64le(30_000_000);
        v.u64le(30_000_000);
        v.u64le(0);
        v.u32le(2);
        v.u32le(256);
        v.u32le(256);
        v.u32le(128_000);
        v.extend_from_slice(&stream);
        v.extend_from_slice(&data_guid);
        v.u64le(data_len);
        v.extend_from_slice(&[7; 16]);
        v.u64le(4);
        v.extend_from_slice(&[1, 1]);
        v.extend_from_slice(&packets);
        files.push(("asf.wmv", v));
    }

    // Audio
    let frame = |n: usize| {
        let mut f = vec![0u8; 417];
        f[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        f[4..].copy_from_slice(&pattern(413, n as u8)[..413]);
        // keep the next frame's sync from appearing inside the payload
        for b in &mut f[4..] {
            if *b == 0xFF {
                *b = 0x7F;
            }
        }
        f
    };
    {
        let title = b"\x00FFR Sample";
        let mut tag = b"TIT2".to_vec();
        tag.u32be(title.len() as u32);
        tag.extend_from_slice(&[0, 0]);
        tag.extend_from_slice(title);
        let mut v = b"ID3".to_vec();
        v.extend_from_slice(&[3, 0, 0]);
        let n = tag.len();
        v.extend_from_slice(&[
            ((n >> 21) & 0x7F) as u8,
            ((n >> 14) & 0x7F) as u8,
            ((n >> 7) & 0x7F) as u8,
            (n & 0x7F) as u8,
        ]);
        v.extend_from_slice(&tag);
        for i in 0..12 {
            v.extend_from_slice(&frame(i));
        }
        files.push(("mp3.mp3", v));
    }
    {
        let mut v = Vec::new();
        for i in 0..12 {
            v.extend_from_slice(&frame(i + 20));
        }
        files.push(("mp3raw.mp3", v));
    }
    {
        let pcm = pattern(4000, 1);
        let mut v = b"RIFF".to_vec();
        v.u32le(36 + pcm.len() as u32);
        v.extend_from_slice(b"WAVEfmt ");
        v.u32le(16);
        v.u16le(1);
        v.u16le(1);
        v.u32le(8000);
        v.u32le(8000);
        v.u16le(1);
        v.u16le(8);
        v.extend_from_slice(b"data");
        v.u32le(pcm.len() as u32);
        v.extend_from_slice(&pcm);
        files.push(("wav.wav", v));
    }
    {
        // FLAC: STREAMINFO + 4 CONSTANT-subframe frames (16-bit mono, 4096 samples each).
        let frames = 4u64;
        let mut v = b"fLaC".to_vec();
        v.push(0x80);
        v.extend_from_slice(&[0, 0, 34]);
        v.u16be(4096);
        v.u16be(4096);
        v.extend_from_slice(&[0, 0, 11, 0, 0, 11]);
        let x: u64 = (44_100u64 << 44) | (15u64 << 36) | (frames * 4096);
        v.u64be(x);
        v.extend_from_slice(&[0; 16]);
        for i in 0..frames {
            let mut f = vec![0xFF, 0xF8, 0xC9, 0x08, i as u8];
            f.push(crc8(&f));
            f.push(0x00);
            f.extend_from_slice(&[0x00, 0x00]);
            let c = crc16_flac(&f);
            f.u16be(c);
            v.extend_from_slice(&f);
        }
        files.push(("flac.flac", v));
    }
    {
        let mut ident = b"\x01vorbis".to_vec();
        ident.extend_from_slice(&[0; 4]);
        ident.push(2);
        ident.u32le(44_100);
        ident.extend_from_slice(&[0; 12]);
        ident.push(0xB8);
        ident.push(1);
        let mut v = ogg_page(0x02, 0, 0, &ident);
        v.extend(ogg_page(0x00, 1, 0, &pattern(300, 2)));
        v.extend(ogg_page(0x04, 2, 44_100, &pattern(120, 3)));
        files.push(("ogg.ogg", v));
    }
    {
        let mut moov = mvhd(3000);
        moov.extend_from_slice(&bmff_box(b"trak", &[]));
        let mut v = ftyp(b"M4A ", &[b"M4A ", b"mp42", b"isom"]);
        v.extend_from_slice(&bmff_box(b"moov", &moov));
        v.extend_from_slice(&bmff_box(b"mdat", &pattern(1800, 11)));
        files.push(("m4a.m4a", v));
    }
    {
        let mut v = Vec::new();
        for i in 0..24 {
            let len = 207usize;
            v.extend_from_slice(&[
                0xFF,
                0xF1,
                0x50,
                0x80 | ((len >> 11) & 3) as u8,
                ((len >> 3) & 0xFF) as u8,
                (((len & 7) << 5) | 0x1F) as u8,
                0xFC,
            ]);
            let mut p = pattern(len - 7, i);
            for b in &mut p {
                if *b == 0xFF {
                    *b = 0x7F;
                }
            }
            v.extend_from_slice(&p);
        }
        files.push(("aac.aac", v));
    }
    {
        let data = pattern(2000, 12);
        let mut body = b"AIFF".to_vec();
        body.extend_from_slice(b"COMM");
        body.u32be(18);
        body.u16be(1);
        body.u32be(2000);
        body.u16be(8);
        body.extend_from_slice(&[0x40, 0x0B, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        body.extend_from_slice(b"SSND");
        body.u32be(8 + data.len() as u32);
        body.u32be(0);
        body.u32be(0);
        body.extend_from_slice(&data);
        let mut v = b"FORM".to_vec();
        v.u32be(body.len() as u32);
        v.extend_from_slice(&body);
        files.push(("aiff.aiff", v));
    }
    {
        let mut v = b"MThd".to_vec();
        v.u32be(6);
        v.u16be(0);
        v.u16be(1);
        v.u16be(96);
        let track = [
            0x00, 0x90, 0x3C, 0x40, 0x60, 0x80, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00,
        ];
        v.extend_from_slice(b"MTrk");
        v.u32be(track.len() as u32);
        v.extend_from_slice(&track);
        files.push(("midi.mid", v));
    }

    // Documents
    {
        let mut v = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offs = Vec::new();
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
            "<< /Length 44 >>\nstream\nBT /F1 12 Tf 20 50 Td (Fast File Recovery) Tj ET\nendstream"
                .to_string(),
        ];
        for (i, o) in objs.iter().enumerate() {
            offs.push(v.len());
            v.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = v.len();
        v.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offs {
            v.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        v.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        files.push(("pdf.pdf", v));
    }
    files.push((
        "zip.zip",
        zip(&[
            ("readme.txt", b"Fast File Recovery sample archive.\n"),
            ("data.bin", &pattern(3000, 13)),
        ]),
    ));
    files.push((
        "zip.docx",
        zip(&[
            ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#),
            ("_rels/.rels", br#"<?xml version="1.0"?><Relationships/>"#),
            ("word/document.xml", br#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body></w:document>"#),
        ]),
    ));
    files.push((
        "zip.epub",
        zip(&[
            ("mimetype", b"application/epub+zip"),
            (
                "META-INF/container.xml",
                br#"<?xml version="1.0"?><container/>"#,
            ),
        ]),
    ));
    {
        // OLE compound file (v3, 512-byte sectors) holding a "WordDocument" stream entry.
        let ss = 512usize;
        let mut v = vec![0u8; ss * 4];
        v[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        v[24..26].copy_from_slice(&0x3Eu16.to_le_bytes());
        v[26..28].copy_from_slice(&3u16.to_le_bytes());
        v[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
        v[30..32].copy_from_slice(&9u16.to_le_bytes());
        v[32..34].copy_from_slice(&6u16.to_le_bytes());
        set32le(&mut v, 44, 1); // one FAT sector
        set32le(&mut v, 48, 1); // directory at sector 1
        set32le(&mut v, 56, 4096);
        set32le(&mut v, 60, 0xFFFF_FFFE);
        set32le(&mut v, 68, 0xFFFF_FFFE);
        for i in 0..109 {
            set32le(&mut v, 76 + i * 4, 0xFFFF_FFFF);
        }
        set32le(&mut v, 76, 0); // FAT in sector 0
                                // FAT (sector 0 → file offset 512)
        let fat = ss;
        for i in 0..128 {
            set32le(&mut v, fat + i * 4, 0xFFFF_FFFF);
        }
        set32le(&mut v, fat, 0xFFFF_FFFD); // FATSECT
        set32le(&mut v, fat + 4, 0xFFFF_FFFE); // directory chain end
        set32le(&mut v, fat + 8, 0xFFFF_FFFE); // stream sector
                                               // Directory (sector 1 → offset 1024)
        let dir = ss * 2;
        let name: Vec<u8> = "Root Entry"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        v[dir..dir + name.len()].copy_from_slice(&name);
        v[dir + 64..dir + 66].copy_from_slice(&((name.len() + 2) as u16).to_le_bytes());
        v[dir + 66] = 5;
        set32le(&mut v, dir + 76, 1);
        let e = dir + 128;
        let name: Vec<u8> = "WordDocument"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        v[e..e + name.len()].copy_from_slice(&name);
        v[e + 64..e + 66].copy_from_slice(&((name.len() + 2) as u16).to_le_bytes());
        v[e + 66] = 2;
        set32le(&mut v, e + 68, 0xFFFF_FFFF);
        set32le(&mut v, e + 72, 0xFFFF_FFFF);
        set32le(&mut v, e + 76, 0xFFFF_FFFF);
        set32le(&mut v, e + 116, 2);
        set32le(&mut v, e + 120, 4096);
        for k in [dir + 256, dir + 384] {
            set32le(&mut v, k + 68, 0xFFFF_FFFF);
            set32le(&mut v, k + 72, 0xFFFF_FFFF);
            set32le(&mut v, k + 76, 0xFFFF_FFFF);
        }
        // stream data (sector 2 → offset 1536)
        let s = ss * 3;
        v[s..s + ss].copy_from_slice(&pattern(ss, 14));
        files.push(("ole.doc", v));
    }
    files.push((
        "rtf.rtf",
        br"{\rtf1\ansi\deff0 {\fonttbl {\f0 Arial;}}\f0 Fast File Recovery {\b sample} with \{escaped\} braces.\par}".to_vec(),
    ));

    // Archives
    {
        let data = pattern(3000, 15);
        let mut v = vec![0x1F, 0x8B, 0x08, 0x08, 0, 0, 0, 0, 0, 3];
        v.extend_from_slice(b"sample.bin\0");
        v.push(0x01); // final stored block
        v.u16le(data.len() as u16);
        v.u16le(!(data.len() as u16));
        v.extend_from_slice(&data);
        v.u32le(crc32(&data));
        v.u32le(data.len() as u32);
        files.push(("gzip.gz", v));
    }
    {
        let data = pattern(1500, 16);
        let mut v = tar_header("sample.bin", data.len());
        v.extend_from_slice(&data);
        v.resize(v.len().div_ceil(512) * 512, 0);
        v.extend_from_slice(&[0u8; 1024]);
        files.push(("tar.tar", v));
    }
    {
        let next = [0x01u8, 0x00];
        let mut v = vec![b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4];
        let mut sh = Vec::new();
        sh.u64le(0);
        sh.u64le(next.len() as u64);
        sh.u32le(crc32(&next));
        v.u32le(crc32(&sh));
        v.extend_from_slice(&sh);
        v.extend_from_slice(&next);
        files.push(("7z.7z", v));
    }
    {
        let data = pattern(800, 17);
        let mut v = b"Rar!\x1A\x07\x01\x00".to_vec();
        v.extend(rar5_header(&[1, 0, 0]));
        let mut fh = vec![2, 0x02];
        fh.extend(vint(data.len() as u64));
        fh.push(0); // file flags
        fh.extend(vint(data.len() as u64));
        fh.push(0x20); // attributes
        fh.push(0); // compression info
        fh.push(0); // host OS
        fh.push(10);
        fh.extend_from_slice(b"sample.bin");
        v.extend(rar5_header(&fh));
        v.extend_from_slice(&data);
        v.extend(rar5_header(&[5, 0, 0]));
        files.push(("rar5.rar", v));
    }
    {
        // XZ with one LZMA2 block holding an uncompressed chunk.
        let data = pattern(2000, 18);
        let mut v = vec![0xFD, b'7', b'z', b'X', b'Z', 0x00, 0x00, 0x01];
        v.u32le(crc32(&[0x00, 0x01]));
        let mut bh = vec![0x02, 0x00, 0x21, 0x01, 0x16, 0, 0, 0];
        let c = crc32(&bh);
        bh.u32le(c);
        let mut comp = vec![0x01];
        comp.u16be((data.len() - 1) as u16);
        comp.extend_from_slice(&data);
        comp.push(0x00);
        // Unpadded size = block header + compressed data + check (CRC32).
        let unpadded = bh.len() + comp.len() + 4;
        v.extend_from_slice(&bh);
        v.extend_from_slice(&comp);
        while (v.len() - 12) % 4 != 0 {
            v.push(0);
        }
        v.u32le(crc32(&data));
        let mut index = vec![0x00];
        index.extend(vint(1));
        index.extend(vint(unpadded as u64));
        index.extend(vint(data.len() as u64));
        while index.len() % 4 != 0 {
            index.push(0);
        }
        let c = crc32(&index);
        index.u32le(c);
        v.extend_from_slice(&index);
        let mut footer = Vec::new();
        footer.u32le((index.len() / 4 - 1) as u32);
        footer.extend_from_slice(&[0x00, 0x01]);
        v.u32le(crc32(&footer));
        v.extend_from_slice(&footer);
        v.extend_from_slice(b"YZ");
        files.push(("xz.xz", v));
    }
    {
        // ISO 9660: 16 empty system sectors, primary volume descriptor, terminator.
        let mut v = vec![0u8; 0x9000];
        let p = 0x8000;
        v[p] = 1;
        v[p + 1..p + 6].copy_from_slice(b"CD001");
        v[p + 6] = 1;
        v[p + 40..p + 72].copy_from_slice(b"FFR_SAMPLE                      ");
        set32le(&mut v, p + 80, 18);
        set32be(&mut v, p + 84, 18);
        v[p + 128..p + 130].copy_from_slice(&2048u16.to_le_bytes());
        v[p + 130..p + 132].copy_from_slice(&2048u16.to_be_bytes());
        let t = 0x8800;
        v[t] = 255;
        v[t + 1..t + 6].copy_from_slice(b"CD001");
        v[t + 6] = 1;
        files.push(("iso.iso", v));
    }
    {
        let data = pattern(600, 19);
        let total = 66 + 8 + data.len();
        let mut v = b"MSCF".to_vec();
        v.u32le(0);
        v.u32le(total as u32);
        v.u32le(0);
        v.u32le(44);
        v.u32le(0);
        v.extend_from_slice(&[3, 1]);
        v.u16le(1);
        v.u16le(1);
        v.u16le(0);
        v.u16le(0);
        v.u16le(0);
        v.u32le(66);
        v.u16le(1);
        v.u16le(0);
        v.u32le(data.len() as u32);
        v.u32le(0);
        v.u16le(0);
        v.u16le(0x5A21);
        v.u16le(0x6000);
        v.u16le(0x20);
        v.extend_from_slice(b"a.txt\0");
        v.u32le(0);
        v.u16le(data.len() as u16);
        v.u16le(data.len() as u16);
        v.extend_from_slice(&data);
        files.push(("cab.cab", v));
    }

    // Other
    {
        let mut v = vec![0u8; 2048];
        v[..16].copy_from_slice(b"SQLite format 3\0");
        v[16..18].copy_from_slice(&1024u16.to_be_bytes());
        v[18] = 1;
        v[19] = 1;
        v[21] = 64;
        v[22] = 32;
        v[23] = 32;
        set32be(&mut v, 24, 1);
        set32be(&mut v, 28, 2);
        set32be(&mut v, 92, 1);
        set32be(&mut v, 96, 3_045_000);
        v[100] = 0x0D;
        files.push(("sqlite.sqlite", v));
    }
    {
        let mut v = vec![0u8; 0x400];
        v[0..2].copy_from_slice(b"MZ");
        set32le(&mut v, 0x3C, 0x40);
        v[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        v[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        v[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        v[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        v[coff + 18..coff + 20].copy_from_slice(&0x0022u16.to_le_bytes());
        let opt = coff + 20;
        v[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        set32le(&mut v, opt + 60, 0x200); // SizeOfHeaders
        set32le(&mut v, opt + 108, 16); // NumberOfRvaAndSizes
        let sect = opt + 240;
        v[sect..sect + 5].copy_from_slice(b".text");
        set32le(&mut v, sect + 8, 0x200);
        set32le(&mut v, sect + 12, 0x1000);
        set32le(&mut v, sect + 16, 0x200);
        set32le(&mut v, sect + 20, 0x200);
        for b in &mut v[0x200..0x400] {
            *b = 0xCC;
        }
        files.push(("pe.exe", v));
    }
    {
        let mut v = vec![0u8; 1024];
        v[..4].copy_from_slice(b"\x7FELF");
        v[4] = 2;
        v[5] = 1;
        v[6] = 1;
        v[16..18].copy_from_slice(&2u16.to_le_bytes());
        v[18..20].copy_from_slice(&0x3Eu16.to_le_bytes());
        set32le(&mut v, 20, 1);
        v[32..40].copy_from_slice(&64u64.to_le_bytes());
        v[52..54].copy_from_slice(&64u16.to_le_bytes());
        v[54..56].copy_from_slice(&56u16.to_le_bytes());
        v[56..58].copy_from_slice(&1u16.to_le_bytes());
        v[58..60].copy_from_slice(&64u16.to_le_bytes());
        let ph = 64;
        set32le(&mut v, ph, 1);
        set32le(&mut v, ph + 4, 5);
        v[ph + 32..ph + 40].copy_from_slice(&1024u64.to_le_bytes());
        v[ph + 40..ph + 48].copy_from_slice(&1024u64.to_le_bytes());
        files.push(("elf.elf", v));
    }
    {
        let mut v = vec![0u8; 4096];
        v[..4].copy_from_slice(&[0xCF, 0xFA, 0xED, 0xFE]);
        set32le(&mut v, 4, 0x0100_000C);
        set32le(&mut v, 12, 2);
        set32le(&mut v, 16, 1);
        set32le(&mut v, 20, 72);
        let c = 32;
        set32le(&mut v, c, 0x19);
        set32le(&mut v, c + 4, 72);
        v[c + 8..c + 14].copy_from_slice(b"__TEXT");
        v[c + 32..c + 40].copy_from_slice(&4096u64.to_le_bytes());
        v[c + 40..c + 48].copy_from_slice(&0u64.to_le_bytes());
        v[c + 48..c + 56].copy_from_slice(&4096u64.to_le_bytes());
        files.push(("macho.macho", v));
    }
    {
        let mut v = vec![0u8; 4096];
        v[..4].copy_from_slice(b"!BDN");
        v[8..10].copy_from_slice(b"SM");
        v[10..12].copy_from_slice(&23u16.to_le_bytes());
        v[0xB8..0xC0].copy_from_slice(&4096u64.to_le_bytes());
        files.push(("pst.pst", v));
    }
    {
        let mut v = vec![0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5];
        v.u16le(0);
        v.u16le(4);
        v.push(2);
        v.u32le(16);
        v.extend_from_slice(&[0x31; 16]);
        v.push(0);
        v.u32le(4);
        v.extend_from_slice(b"\r\n\r\n");
        v.extend_from_slice(&[0x5A; 64]);
        v.extend_from_slice(&[0x11; 32]);
        v.u32le(64);
        v.extend_from_slice(&pattern(64, 20));
        v.extend_from_slice(&[0x22; 32]);
        v.u32le(0);
        files.push(("kdbx.kdbx", v));
    }
    {
        let mut footer = vec![0u8; 512];
        footer[..8].copy_from_slice(b"conectix");
        footer[16..24].copy_from_slice(&512u64.to_be_bytes());
        set32be(&mut footer, 60, 3);
        let mut v = footer.clone();
        let mut dynh = vec![0u8; 1024];
        dynh[..8].copy_from_slice(b"cxsparse");
        dynh[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
        dynh[16..24].copy_from_slice(&1536u64.to_be_bytes());
        set32be(&mut dynh, 28, 4);
        set32be(&mut dynh, 32, 4096);
        v.extend_from_slice(&dynh);
        let mut bat = vec![0xFFu8; 512];
        set32be(&mut bat, 0, 4);
        v.extend_from_slice(&bat);
        v.extend_from_slice(&[0xFF; 512]);
        v.extend_from_slice(&pattern(4096, 21));
        v.extend_from_slice(&footer);
        files.push(("vhd.vhd", v));
    }
    {
        let mut v = Vec::new();
        v.u32be(0x0001_0000);
        v.u16be(2);
        v.u16be(32);
        v.u16be(1);
        v.u16be(0);
        for (tag, off, len) in [(b"head", 44u32, 54u32), (b"name", 100, 300)] {
            v.extend_from_slice(tag);
            v.u32be(0);
            v.u32be(off);
            v.u32be(len);
        }
        v.resize(44, 0);
        v.extend_from_slice(&pattern(54, 22));
        v.resize(100, 0);
        v.extend_from_slice(&pattern(300, 23));
        files.push(("ttf.ttf", v));
    }
    {
        let mut v = b"wOFF".to_vec();
        v.u32be(0x0001_0000);
        v.u32be(160);
        v.u16be(1);
        v.u16be(0);
        v.resize(160, 0x2A);
        files.push(("woff.woff", v));
    }
    files.push((
        "xml.xml",
        br#"<?xml version="1.0" encoding="UTF-8"?>
<recovery tool="Fast File Recovery"><item name="sample">This is a text sample.</item></recovery>
"#
        .to_vec(),
    ));

    for (name, data) in &files {
        if data.len() > 50 * 1024 {
            return Err(format!("{name} is larger than 50 KiB"));
        }
        std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
    }
    println!("wrote {} samples to {}", files.len(), dir.display());
    Ok(())
}
