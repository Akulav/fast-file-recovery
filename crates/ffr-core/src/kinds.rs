//! File kinds: the extension and category of every result. Hits store a compact `u16` index
//! into [`KINDS`].

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    Images,
    Video,
    Audio,
    Documents,
    Archives,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Images,
        Category::Video,
        Category::Audio,
        Category::Documents,
        Category::Archives,
        Category::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Images => "Images",
            Category::Video => "Video",
            Category::Audio => "Audio",
            Category::Documents => "Documents",
            Category::Archives => "Archives",
            Category::Other => "Other",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Kind {
    pub ext: &'static str,
    pub category: Category,
    pub description: &'static str,
}

const fn k(ext: &'static str, category: Category, description: &'static str) -> Kind {
    Kind {
        ext,
        category,
        description,
    }
}

use Category::*;

/// Every known kind. Index 0 is the generic "unknown" kind.
pub static KINDS: &[Kind] = &[
    k("bin", Other, "Unknown file"),
    // Images
    k("jpg", Images, "JPEG image"),
    k("png", Images, "PNG image"),
    k("gif", Images, "GIF image"),
    k("bmp", Images, "Bitmap image"),
    k("tif", Images, "TIFF image"),
    k("dng", Images, "Adobe DNG raw photo"),
    k("cr2", Images, "Canon raw photo"),
    k("cr3", Images, "Canon raw photo"),
    k("nef", Images, "Nikon raw photo"),
    k("arw", Images, "Sony raw photo"),
    k("webp", Images, "WebP image"),
    k("heic", Images, "HEIC image"),
    k("avif", Images, "AVIF image"),
    k("psd", Images, "Photoshop document"),
    k("psb", Images, "Photoshop large document"),
    k("ico", Images, "Icon"),
    k("svg", Images, "SVG image"),
    // Video
    k("mp4", Video, "MP4 video"),
    k("mov", Video, "QuickTime video"),
    k("m4v", Video, "MPEG-4 video"),
    k("3gp", Video, "3GP video"),
    k("avi", Video, "AVI video"),
    k("mkv", Video, "Matroska video"),
    k("webm", Video, "WebM video"),
    k("flv", Video, "Flash video"),
    k("ts", Video, "MPEG transport stream"),
    k("wmv", Video, "Windows Media video"),
    // Audio
    k("mp3", Audio, "MP3 audio"),
    k("wav", Audio, "WAV audio"),
    k("flac", Audio, "FLAC audio"),
    k("ogg", Audio, "Ogg audio"),
    k("opus", Audio, "Opus audio"),
    k("m4a", Audio, "MPEG-4 audio"),
    k("aac", Audio, "AAC audio"),
    k("aiff", Audio, "AIFF audio"),
    k("mid", Audio, "MIDI"),
    k("wma", Audio, "Windows Media audio"),
    // Documents
    k("pdf", Documents, "PDF document"),
    k("docx", Documents, "Word document"),
    k("xlsx", Documents, "Excel workbook"),
    k("pptx", Documents, "PowerPoint presentation"),
    k("odt", Documents, "OpenDocument text"),
    k("ods", Documents, "OpenDocument spreadsheet"),
    k("odp", Documents, "OpenDocument presentation"),
    k("epub", Documents, "E-book"),
    k("doc", Documents, "Word 97-2003 document"),
    k("xls", Documents, "Excel 97-2003 workbook"),
    k("ppt", Documents, "PowerPoint 97-2003"),
    k("msg", Documents, "Outlook message"),
    k("ole", Documents, "Office compound file"),
    k("rtf", Documents, "Rich text"),
    k("xml", Documents, "XML"),
    k("html", Documents, "HTML page"),
    k("txt", Documents, "Text"),
    k("csv", Documents, "CSV table"),
    k("md", Documents, "Markdown"),
    k("json", Documents, "JSON"),
    // Archives
    k("zip", Archives, "ZIP archive"),
    k("rar", Archives, "RAR archive"),
    k("7z", Archives, "7-Zip archive"),
    k("gz", Archives, "Gzip archive"),
    k("bz2", Archives, "Bzip2 archive"),
    k("xz", Archives, "XZ archive"),
    k("tar", Archives, "TAR archive"),
    k("iso", Archives, "ISO disc image"),
    k("cab", Archives, "Cabinet archive"),
    k("jar", Archives, "Java archive"),
    k("apk", Archives, "Android package"),
    // Other
    k("sqlite", Other, "SQLite database"),
    k("exe", Other, "Windows program"),
    k("dll", Other, "Windows library"),
    k("elf", Other, "Linux program"),
    k("macho", Other, "macOS program"),
    k("eml", Other, "E-mail message"),
    k("pst", Other, "Outlook data file"),
    k("ost", Other, "Outlook offline data"),
    k("kdbx", Other, "KeePass database"),
    k("vmdk", Other, "VMware disk"),
    k("vhd", Other, "Virtual hard disk"),
    k("vhdx", Other, "Virtual hard disk"),
    k("ttf", Other, "TrueType font"),
    k("otf", Other, "OpenType font"),
    k("woff", Other, "Web font"),
    k("woff2", Other, "Web font"),
];

/// Extension aliases mapped to their canonical kind (used for quick-scan names).
static ALIASES: &[(&str, &str)] = &[
    ("jpeg", "jpg"),
    ("jpe", "jpg"),
    ("jfif", "jpg"),
    ("tiff", "tif"),
    ("heif", "heic"),
    ("qt", "mov"),
    ("mpg", "ts"),
    ("mpeg", "ts"),
    ("m2ts", "ts"),
    ("mts", "ts"),
    ("aif", "aiff"),
    ("midi", "mid"),
    ("htm", "html"),
    ("tgz", "gz"),
    ("db", "sqlite"),
    ("sqlite3", "sqlite"),
    ("log", "txt"),
    ("ini", "txt"),
    ("tbz2", "bz2"),
];

pub const UNKNOWN: u16 = 0;

/// Kind index for an extension (case-insensitive). Returns [`UNKNOWN`] when not known.
pub fn lookup(ext: &str) -> u16 {
    let lower = ext.to_ascii_lowercase();
    let canon = ALIASES
        .iter()
        .find(|(a, _)| *a == lower)
        .map(|(_, c)| *c)
        .unwrap_or(lower.as_str());
    KINDS
        .iter()
        .position(|k| k.ext == canon)
        .map(|i| i as u16)
        .unwrap_or(UNKNOWN)
}

/// The kind for an index (falls back to unknown).
pub fn get(index: u16) -> &'static Kind {
    KINDS.get(index as usize).unwrap_or(&KINDS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        assert_eq!(get(lookup("JPEG")).ext, "jpg");
        assert_eq!(get(lookup("docx")).category, Category::Documents);
        assert_eq!(lookup("nope"), UNKNOWN);
    }

    #[test]
    fn exts_unique() {
        let mut v: Vec<_> = KINDS.iter().map(|k| k.ext).collect();
        v.sort();
        let n = v.len();
        v.dedup();
        assert_eq!(n, v.len());
    }
}
