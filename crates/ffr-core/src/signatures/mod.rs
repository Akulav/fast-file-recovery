//! The signature registry. It is data, not scattered code: adding a format means one entry
//! here, one resolver in `carve/`, and one test.

mod archives;
mod audio;
mod documents;
mod images;
mod other;
mod video;

use crate::carve::{Carved, Reader};
use crate::kinds::Category;
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub(crate) const KB: u64 = 1024;
pub(crate) const MB: u64 = 1024 * KB;
pub(crate) const GB: u64 = 1024 * MB;

#[derive(Clone, Copy)]
pub enum SizeResolver {
    /// Parse the format's own structure.
    Structured(fn(&mut Reader, u64) -> Option<Carved>),
    /// Search forward for a footer.
    Footer {
        footer: &'static [u8],
        include_footer: bool,
        search_limit: u64,
    },
    /// Size stored in the header (gets up to the first 4 KiB).
    HeaderSize(fn(&[u8]) -> Option<u64>),
    Fixed(u64),
    /// Last resort, confidence Low.
    NextHeaderOrMax,
}

pub struct Signature {
    pub id: &'static str,
    pub name: &'static str,
    pub ext: &'static str,
    pub category: Category,
    pub magic: &'static [&'static [u8]],
    /// Where the magic sits inside the file (MP4 `ftyp` at +4, TAR `ustar` at +257, …).
    pub magic_offset: u64,
    pub min_size: u64,
    pub max_size: u64,
    pub resolver: SizeResolver,
    /// Accept only at 512-byte aligned starts when "Only cluster-aligned starts" is on.
    pub sector_aligned: bool,
    pub default_enabled: bool,
}

/// Every signature, in a fixed order (hits store the index).
pub fn all() -> &'static [Signature] {
    static ALL: OnceLock<Vec<Signature>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v = Vec::new();
        for fam in [
            images::sigs(),
            video::sigs(),
            audio::sigs(),
            documents::sigs(),
            archives::sigs(),
            other::sigs(),
        ] {
            v.extend(fam);
        }
        v
    })
}

pub fn by_id(id: &str) -> Option<(usize, &'static Signature)> {
    all().iter().enumerate().find(|(_, s)| s.id == id)
}

/// Ids of signatures that are on by default.
pub fn default_ids() -> BTreeSet<String> {
    all()
        .iter()
        .filter(|s| s.default_enabled)
        .map(|s| s.id.to_string())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    PhotosVideos,
    Documents,
    Everything,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::PhotosVideos, Preset::Documents, Preset::Everything];

    pub fn label(self) -> &'static str {
        match self {
            Preset::PhotosVideos => "Photos & videos",
            Preset::Documents => "Documents",
            Preset::Everything => "Everything",
        }
    }

    pub fn ids(self) -> BTreeSet<String> {
        all()
            .iter()
            .filter(|s| match self {
                Preset::PhotosVideos => {
                    s.default_enabled && matches!(s.category, Category::Images | Category::Video)
                }
                Preset::Documents => {
                    s.default_enabled
                        && (s.category == Category::Documents
                            || matches!(s.id, "zip" | "rar4" | "rar5" | "7z"))
                }
                Preset::Everything => s.default_enabled,
            })
            .map(|s| s.id.to_string())
            .collect()
    }
}

/// Longest magic end (`magic_offset + len`) of the enabled set.
pub fn max_magic_len(sigs: &[usize]) -> usize {
    let all = all();
    sigs.iter()
        .flat_map(|&i| all[i].magic.iter().map(|m| m.len()))
        .max()
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        let all = all();
        assert!(all.len() > 40);
        let mut ids: Vec<_> = all.iter().map(|s| s.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len(), "duplicate signature ids");
        for s in all {
            assert!(
                !s.magic.is_empty() && s.magic.iter().all(|m| !m.is_empty()),
                "{}",
                s.id
            );
            assert!(s.min_size <= s.max_size, "{}", s.id);
            assert_ne!(
                crate::kinds::lookup(s.ext),
                crate::kinds::UNKNOWN,
                "{}",
                s.ext
            );
        }
        assert!(all.len() < u16::MAX as usize);
    }

    #[test]
    fn presets() {
        assert!(Preset::PhotosVideos.ids().contains("jpeg"));
        assert!(!Preset::PhotosVideos.ids().contains("pdf"));
        assert!(Preset::Documents.ids().contains("pdf"));
        assert!(!Preset::Everything.ids().contains("xml"));
    }
}
