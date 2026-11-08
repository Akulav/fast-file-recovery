//! Every sample in `tests/samples` (`<signature id>.<expected ext>`) must resolve to exactly
//! its own length, with the right sub-type.

use ffr_core::carve::{self, Reader};
use ffr_core::io::MemSource;
use ffr_core::{kinds, signatures};
use std::path::{Path, PathBuf};

fn samples() -> Vec<(PathBuf, String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .expect("samples folder")
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            let (stem, ext) = name.split_once('.')?;
            Some((p.clone(), stem.to_string(), ext.to_string()))
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_sample_resolves_exactly() {
    let mut count = 0;
    for (path, sig_id, ext) in samples() {
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() <= 50 * 1024, "{path:?} is over 50 KiB");
        let (_, sig) = signatures::by_id(&sig_id)
            .unwrap_or_else(|| panic!("no signature '{sig_id}' for {path:?}"));
        let start = 4096usize;
        let mut buf = vec![0u8; start + data.len() + 8192];
        buf[start..start + data.len()].copy_from_slice(&data);
        let src = MemSource::new(buf);
        let mut r = Reader::new(&src, 0, &[]);
        let res = carve::resolve(sig, &mut r, start as u64)
            .unwrap_or_else(|| panic!("{path:?} was not recognised"));
        assert_eq!(res.len, data.len() as u64, "{path:?} length");
        assert!(!res.truncated, "{path:?} flagged truncated");
        assert_eq!(kinds::get(res.kind).ext, ext, "{path:?} sub-type");
        // The magic must be at the signature's offset.
        let m = sig.magic_offset as usize;
        assert!(
            sig.magic
                .iter()
                .any(|mg| data.get(m..m + mg.len()) == Some(*mg)),
            "{path:?} magic"
        );
        count += 1;
    }
    assert!(count >= 40, "only {count} samples");
}

#[test]
fn truncated_file_at_device_end_is_flagged() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples/png.png");
    let data = std::fs::read(path).unwrap();
    let cut = data.len() - 200;
    let mut buf = vec![0u8; 4096];
    buf.extend_from_slice(&data[..cut]);
    let src = MemSource::new(buf);
    let (_, sig) = signatures::by_id("png").unwrap();
    let mut r = Reader::new(&src, 0, &[]);
    let res = carve::resolve(sig, &mut r, 4096).expect("truncated hit");
    assert!(res.truncated);
    assert_eq!(res.len, cut as u64);
}

#[test]
fn garbage_is_rejected() {
    // Random bytes behind every magic must not produce a confident hit.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut noise = vec![0u8; 1 << 20];
    for b in noise.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    for sig in signatures::all() {
        if matches!(sig.resolver, signatures::SizeResolver::NextHeaderOrMax) {
            continue;
        }
        let mut buf = noise.clone();
        let m = sig.magic[0];
        let at = 4096 + sig.magic_offset as usize;
        buf[at..at + m.len()].copy_from_slice(m);
        let src = MemSource::new(buf);
        let mut r = Reader::new(&src, 0, &[]);
        if let Some(res) = carve::resolve(sig, &mut r, 4096) {
            assert!(
                res.confidence < carve::Confidence::High || res.len < 64 * 1024,
                "{} accepted noise as a {} byte file",
                sig.id,
                res.len
            );
        }
    }
}
