//! Session files: `postcard` + zstd (via the pure-Rust `ruzstd`, so no C compiler is needed),
//! with a magic and format version.

use super::Session;
use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAGIC: [u8; 8] = *b"FFRSCAN\0";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    session: Session,
}

/// Save atomically (write to a temp file, then rename).
pub fn save_session(path: &Path, session: &Session) -> Result<()> {
    let file = SessionFile {
        version: VERSION,
        session: session.clone(),
    };
    let raw = postcard::to_stdvec(&file).map_err(|e| CoreError::Session(e.to_string()))?;
    let packed =
        ruzstd::encoding::compress_to_vec(&raw[..], ruzstd::encoding::CompressionLevel::Fastest);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("ffrscan.tmp");
    let mut data = Vec::with_capacity(packed.len() + 8);
    data.extend_from_slice(&MAGIC);
    data.extend_from_slice(&packed);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_session(path: &Path) -> Result<Session> {
    let data = std::fs::read(path)?;
    if data.len() < 8 || data[..8] != MAGIC {
        return Err(CoreError::Session(
            "not a Fast File Recovery scan file".into(),
        ));
    }
    let mut dec = ruzstd::decoding::StreamingDecoder::new(&data[8..])
        .map_err(|e| CoreError::Session(e.to_string()))?;
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut dec, &mut raw)
        .map_err(|e| CoreError::Session(e.to_string()))?;
    let file: SessionFile =
        postcard::from_bytes(&raw).map_err(|e| CoreError::Session(e.to_string()))?;
    if file.version > VERSION {
        return Err(CoreError::Session(
            "made by a newer version; update the app first".into(),
        ));
    }
    Ok(file.session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::image_device;
    use crate::scan::{Hit, HitExtra, ScanMode, SourceSpec};

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("ffr-sess-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("a.img");
        std::fs::write(&img, vec![0u8; 8192]).unwrap();
        let dev = image_device(&img).unwrap();
        let mut s = Session::new(
            "x".into(),
            SourceSpec {
                device: dev,
                range: None,
            },
            ScanMode::Deep,
        );
        s.hits.push(Hit {
            offset: 512,
            len: 100,
            extra: 0,
            kind: 1,
            sig: 0,
            confidence: crate::carve::Confidence::High,
            flags: 0,
        });
        s.extras.push(HitExtra {
            name: Some("a.jpg".into()),
            ..Default::default()
        });
        s.scanned.push(0..4096);
        let p = dir.join("s.ffrscan");
        save_session(&p, &s).unwrap();
        let l = load_session(&p).unwrap();
        assert_eq!(l.hits, s.hits);
        assert_eq!(l.extras, s.extras);
        assert_eq!(l.scanned, s.scanned);
        std::fs::remove_dir_all(&dir).ok();
    }
}
