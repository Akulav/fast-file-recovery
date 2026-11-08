# Changelog

All notable changes to Fast File Recovery are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-11-08

### Changed
- The license is now the Akulav Source-Available License 1.0: use the releases, read and modify the code privately, but no redistribution or commercial use.
- The README ends with the app icon and a "Made by Akulav" link.

## [0.1.0] - 2026-11-08

### Added
- First release of Fast File Recovery, one program per OS: Windows 10/11 setup, Linux x64 and macOS Apple Silicon binaries that install and update themselves.
- Disk list with model, size, bus, partitions, system-disk and removable badges; scan one partition by clicking it; disk image files work like disks.
- Read-only access to the source disk, which is locked for the length of a scan (Windows volume lock, Linux read-only flag, macOS unmount).
- Quick scan of deleted NTFS, FAT12/16/32 and exFAT entries with original names, folders, timestamps and exact extents.
- Deep scan by file signatures across all CPU cores, with bad-sector retries, a live sector map, throughput graph, pause/resume and resumable checkpoints.
- Over 45 format families across images, video, audio, documents, archives and more, each parsed to find the exact file end.
- Results with category and type filters, size and confidence filters, search, sorting, image thumbnails, text and hex preview, and multi-select.
- Recovery to another disk only, with destination checks, three folder layouts, original timestamps and a CSV/JSON report with BLAKE3 hashes.
- Disk image creation for failing drives, with a map of unreadable areas.
- Save and open scans.
- Dark and light themes, keyboard shortcuts, verified one-click updates, About screen.
- Project guide (`CLAUDE.md`): the architecture, recovery engine design, UI design system, install/update model, release workflow and implementation plan.
