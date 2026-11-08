//! Size resolvers and validators: given a signature hit, find where the file ends.
//!
//! All disk bytes are hostile. Every resolver is bounds-checked, works through a bounded
//! [`Reader`], and gives up (returns `None`) rather than guessing wildly.

pub mod archives;
pub mod audio;
pub mod documents;
pub mod images;
pub mod other;
mod reader;
pub mod video;

pub use reader::Reader;

use crate::kinds;
use crate::signatures::{Signature, SizeResolver};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Confidence {
    Low,
    #[default]
    Medium,
    High,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Low => "Low",
            Confidence::Medium => "Medium",
            Confidence::High => "High",
        }
    }
}

/// Extra details found while parsing (shown in the preview panel and used in names).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u32,
    pub title: Option<String>,
}

impl FileMeta {
    pub fn dims(w: u32, h: u32) -> Option<FileMeta> {
        (w > 0 && h > 0 && w < 100_000 && h < 100_000).then(|| FileMeta {
            width: w,
            height: h,
            ..Default::default()
        })
    }
    pub fn duration(ms: u64) -> Option<FileMeta> {
        (ms > 0).then(|| FileMeta {
            duration_ms: ms.min(u32::MAX as u64) as u32,
            ..Default::default()
        })
    }
    pub fn title(t: String) -> Option<FileMeta> {
        let t: String = t.chars().filter(|c| !c.is_control()).take(80).collect();
        let t = t.trim().to_string();
        (!t.is_empty()).then(|| FileMeta {
            title: Some(t),
            ..Default::default()
        })
    }
}

/// What a resolver found.
#[derive(Clone, Debug)]
pub struct Carved {
    pub len: u64,
    pub confidence: Confidence,
    pub meta: Option<FileMeta>,
    /// Sub-type chosen while parsing (e.g. `docx` for a ZIP), overriding the signature's ext.
    pub ext: Option<&'static str>,
}

impl Carved {
    pub fn new(len: u64, confidence: Confidence) -> Self {
        Self {
            len,
            confidence,
            meta: None,
            ext: None,
        }
    }
    pub fn meta(mut self, m: Option<FileMeta>) -> Self {
        self.meta = m;
        self
    }
    pub fn ext(mut self, e: &'static str) -> Self {
        self.ext = Some(e);
        self
    }
}

/// A resolved hit, clamped to the signature's bounds and the device size.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub len: u64,
    pub confidence: Confidence,
    pub meta: Option<FileMeta>,
    pub kind: u16,
    pub truncated: bool,
}

/// Run a signature's resolver at `start` and clamp the result.
pub fn resolve(sig: &Signature, r: &mut Reader, start: u64) -> Option<Resolved> {
    let size = r.size();
    if start >= size {
        return None;
    }
    r.take_eof();
    let carved = match sig.resolver {
        SizeResolver::Structured(f) => f(r, start),
        SizeResolver::Footer {
            footer,
            include_footer,
            search_limit,
        } => {
            let from = start + sig.magic_offset + sig.magic.first().map_or(1, |m| m.len()) as u64;
            r.find(from, search_limit.min(sig.max_size), footer)
                .map(|p| {
                    let end = if include_footer {
                        p + footer.len() as u64
                    } else {
                        p
                    };
                    Carved::new(end - start, Confidence::Medium)
                })
        }
        SizeResolver::HeaderSize(f) => {
            let n = (size - start).min(4096) as usize;
            r.vec(start, n)
                .and_then(|h| f(&h))
                .map(|len| Carved::new(len, Confidence::Medium))
        }
        SizeResolver::Fixed(n) => Some(Carved::new(n, Confidence::Low)),
        SizeResolver::NextHeaderOrMax => Some(Carved::new(sig.max_size, Confidence::Low)),
    };
    let ext = |c: &Option<Carved>| c.as_ref().and_then(|c| c.ext).unwrap_or(sig.ext);
    let kind = kinds::lookup(ext(&carved));
    let Some(c) = carved else {
        // The structure ran off the end of the device: keep what is there, flagged.
        if r.take_eof() && size - start >= sig.min_size.max(512) {
            return Some(Resolved {
                len: (size - start).min(sig.max_size),
                confidence: Confidence::Low,
                meta: None,
                kind,
                truncated: true,
            });
        }
        return None;
    };
    let mut len = c.len;
    let mut confidence = c.confidence;
    let mut truncated = false;
    if len < sig.min_size || len == 0 {
        return None;
    }
    if len > sig.max_size {
        len = sig.max_size;
        confidence = Confidence::Low;
    }
    if start + len > size {
        len = size - start;
        truncated = !matches!(sig.resolver, SizeResolver::NextHeaderOrMax);
        confidence = Confidence::Low;
    }
    Some(Resolved {
        len,
        confidence,
        meta: c.meta,
        kind,
        truncated,
    })
}

/// True if `b` looks like a printable ASCII four-character code.
pub(crate) fn is_fourcc(b: &[u8]) -> bool {
    b.len() == 4 && b.iter().all(|&c| (0x20..0x7F).contains(&c))
}

/// Length of a run of text-like bytes (UTF-8, tabs, newlines) from `start`, up to `limit`.
pub(crate) fn text_run(r: &mut Reader, start: u64, limit: u64) -> u64 {
    let mut len = 0u64;
    r.scan(start, limit, 0, |_, piece| {
        for &c in piece {
            let ok =
                c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0C || (c >= 0x20 && c != 0x7F);
            if !ok {
                return false;
            }
            len += 1;
        }
        true
    });
    len
}
