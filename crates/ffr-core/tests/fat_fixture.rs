//! FAT32 fixture built with the `fatfs` crate: write files, delete one, quick-scan it back.

use ffr_core::device::{FsType, PartitionInfo};
use ffr_core::fs::{fat::Fat, QuickFile, QuickScanner, ScanControl};
use ffr_core::io::{read_full, MemSource};
use std::io::{Cursor, Write};

struct NoCtl;
impl ScanControl for NoCtl {
    fn should_stop(&self) -> bool {
        false
    }
    fn progress(&self, _: f32) {}
}

fn build(fat_type: fatfs::FatType, size: usize) -> (Vec<u8>, Vec<u8>) {
    let mut img = vec![0u8; size];
    fatfs::format_volume(
        Cursor::new(&mut img[..]),
        fatfs::FormatVolumeOptions::new()
            .fat_type(fat_type)
            .bytes_per_cluster(512),
    )
    .unwrap();
    let content: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect();
    {
        let fs =
            fatfs::FileSystem::new(Cursor::new(&mut img[..]), fatfs::FsOptions::new()).unwrap();
        let root = fs.root_dir();
        root.create_dir("photos").unwrap();
        let mut f = root.create_file("photos/holiday-photo.jpg").unwrap();
        f.write_all(&content).unwrap();
        drop(f);
        let mut k = root.create_file("keep.txt").unwrap();
        k.write_all(b"stay").unwrap();
        drop(k);
        root.remove("photos/holiday-photo.jpg").unwrap();
        drop(root);
        fs.unmount().unwrap();
    }
    (img, content)
}

fn scan(img: Vec<u8>, fs: FsType) -> (MemSource, Vec<QuickFile>) {
    let len = img.len() as u64;
    let src = MemSource::new(img);
    let part = PartitionInfo {
        index: 0,
        offset: 0,
        len,
        fs,
        label: None,
        mount_points: vec![],
    };
    let mut found = Vec::new();
    Fat.scan(&src, &part, &NoCtl, &mut |q| found.push(q))
        .unwrap();
    (src, found)
}

fn check(fat_type: fatfs::FatType, fs: FsType, size: usize) {
    let (img, content) = build(fat_type, size);
    let (src, found) = scan(img, fs);
    let f = found
        .iter()
        .find(|q| q.name == "holiday-photo.jpg")
        .unwrap_or_else(|| {
            panic!(
                "deleted file not found: {:?}",
                found.iter().map(|q| &q.name).collect::<Vec<_>>()
            )
        });
    assert_eq!(f.path, "photos");
    assert_eq!(f.size, content.len() as u64);
    assert!(
        !found.iter().any(|q| q.name == "keep.txt"),
        "live file reported as deleted"
    );
    let (off, len) = f.extents[0];
    let mut buf = vec![0u8; len as usize];
    read_full(&src, off, &mut buf).unwrap();
    assert_eq!(buf, content);
}

#[test]
fn fat32_deleted_file_with_long_name() {
    check(fatfs::FatType::Fat32, FsType::Fat32, 40 << 20);
}

#[test]
fn fat16_deleted_file_with_long_name() {
    check(fatfs::FatType::Fat16, FsType::Fat16, 16 << 20);
}
