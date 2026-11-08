//! Synthetic disk round trip (slow, ignored by default):
//! `cargo test -p ffr-core --release -- --ignored`
//!
//! Builds a 512 MiB image of random noise, places every sample at aligned, unaligned and
//! chunk-crossing offsets (one truncated at the end), runs a deep scan and checks that every
//! whole sample is found with the exact length and BLAKE3, the truncated one is flagged, and
//! nothing is reported twice.

use ffr_core::device::image_device;
use ffr_core::scan::{flags, Hit, ScanEvent, ScanMode, ScanOptions, Session, SourceSpec};
use ffr_core::{kinds, start_scan};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SIZE: u64 = 512 << 20;
const CHUNK: u64 = 16 << 20;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
#[ignore]
fn synthetic_disk_roundtrip() {
    let dir = std::env::temp_dir().join(format!("ffr-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img_path = dir.join("disk.img");
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    {
        let mut f = std::fs::File::create(&img_path).unwrap();
        let mut block = vec![0u8; 4 << 20];
        for _ in 0..SIZE / block.len() as u64 {
            for c in block.as_chunks_mut::<8>().0 {
                c.copy_from_slice(&rng.next().to_le_bytes());
            }
            f.write_all(&block).unwrap();
        }
    }
    let samples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples");
    let mut samples: Vec<(String, Vec<u8>)> = std::fs::read_dir(&samples_dir)
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    samples.sort();

    let mut placed: Vec<(u64, String, Vec<u8>)> = Vec::new();
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&img_path)
            .unwrap();
        let mut pos: u64 = 1 << 20;
        for (i, (name, data)) in samples.iter().enumerate() {
            let at = match i % 3 {
                0 => pos.div_ceil(4096) * 4096,       // cluster aligned
                1 => pos.div_ceil(4096) * 4096 + 7,   // unaligned
                _ => (pos / CHUNK + 1) * CHUNK - 100, // crossing a chunk boundary
            };
            f.seek(SeekFrom::Start(at)).unwrap();
            f.write_all(data).unwrap();
            // A short zero guard so formats without a length field end cleanly.
            f.write_all(&[0u8; 512]).unwrap();
            placed.push((at, name.clone(), data.clone()));
            pos = at + data.len() as u64 + 64 * 1024;
        }
        // Truncated PNG at the very end.
        let png = samples
            .iter()
            .find(|(n, _)| n == "png.png")
            .unwrap()
            .1
            .clone();
        // Round up so at least 300 bytes of the file are cut off.
        let at = (SIZE - (png.len() as u64 - 300)).div_ceil(512) * 512;
        f.seek(SeekFrom::Start(at)).unwrap();
        f.write_all(&png[..(SIZE - at) as usize]).unwrap();
        placed.push((at, "TRUNCATED.png".into(), Vec::new()));
    }

    let dev = image_device(&img_path).unwrap();
    let opts = ScanOptions {
        mode: ScanMode::Deep,
        enabled: ffr_core::signatures::all()
            .iter()
            .map(|s| s.id.to_string())
            .collect(),
        cluster_aligned_only: false,
        lock: false,
        checkpoint_dir: None,
        ..Default::default()
    };
    let source = SourceSpec {
        device: dev.clone(),
        range: None,
    };
    let (_h, rx) = start_scan(source.clone(), opts);
    let mut session = Session::new("t".into(), source, ScanMode::Deep);
    loop {
        match rx.recv().unwrap() {
            ScanEvent::Hits(b) => session.apply(b),
            ScanEvent::Done(_) => break,
            ScanEvent::Failed(e) => panic!("scan failed: {e}"),
            _ => {}
        }
    }

    let mut seen = HashSet::new();
    for h in &session.hits {
        assert!(
            seen.insert((h.offset, h.kind)),
            "duplicate hit at {}",
            h.offset
        );
    }
    let mut img = std::fs::File::open(&img_path).unwrap();
    for (at, name, data) in &placed {
        let ext = name.split_once('.').unwrap().1;
        let hit: Option<&Hit> = session
            .hits
            .iter()
            .find(|h| h.offset == *at && kinds::get(h.kind).ext == ext);
        let hit = hit.unwrap_or_else(|| panic!("{name} at {at} not found"));
        if name.starts_with("TRUNCATED") {
            assert!(hit.has(flags::TRUNCATED), "{name} not flagged truncated");
            continue;
        }
        assert_eq!(hit.len, data.len() as u64, "{name} length");
        let mut got = vec![0u8; hit.len as usize];
        img.seek(SeekFrom::Start(hit.offset)).unwrap();
        img.read_exact(&mut got).unwrap();
        assert_eq!(blake3::hash(&got), blake3::hash(data), "{name} content");
    }
    std::fs::remove_dir_all(&dir).ok();
}
