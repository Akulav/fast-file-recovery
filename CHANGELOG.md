# Changelog

All notable changes to Fast File Recovery are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.3] - 2026-11-12

### Changed
- Everything the app saves now lives in the folder you installed it to: settings, saved scans, logs and downloaded
  updates. Nothing goes to `%APPDATA%`, `~/.config`, `~/.local/state` or the temp folder any more. Settings and saved
  scans from earlier versions are moved in on the first start, also when the install folder is on another drive.
- Linux and macOS: installing asks which folder to use (default `~/.local/share/fast-file-recovery` or
  `~/Applications/Fast File Recovery`), and a newer download offers to update the copy in that folder. An existing
  install is picked up automatically; on macOS the app moves from `~/Applications/Fast File Recovery.app` into
  `~/Applications/Fast File Recovery/`.
- Windows: the setup notes that all data is stored in the chosen folder, updates always install into the same folder,
  and the uninstaller offers to delete settings, saved scans and logs from it.
- One build workflow, the same as the other Akulav apps: every push is checked and built for all three systems, and a
  new version is released automatically. The Linux build now runs on older distributions too (glibc 2.28+).
- New README in the same style as the other Akulav apps.

### Added
- `--portable` (or an empty `portable.txt` next to the program) keeps data next to the program and never offers to
  install.

## [0.1.2] - 2026-11-10

### Changed
- Cleaner, more polished look: softer dividers, gentle card shadows, disk cards that lift on hover and show a check mark when picked, buttons with a subtle glow, a refined segmented control and toggle, banners and notifications with a coloured edge, and a tidier sidebar with section labels and step lines that turn green as you go.
- Buttons and toggles show a focus ring when you move through them with the keyboard.

### Fixed
- Recovery now checks that the connected disk is really the one the scan was made from. After a reboot or re-plug, disk numbers can change; before, a reopened scan could read the wrong disk and the "different disk" check could pass for the very disk being recovered.
- The destination is checked again right before recovery starts, not only when it was picked.
- Two recoveries started in the same second no longer share a folder, and recovery never overwrites an existing file.
- Names in the CSV report that start with `=`, `+`, `-` or `@` can no longer run as formulas in spreadsheet apps.
- Linux/macOS: when running as administrator, giving recovered files back to you no longer follows symbolic links, so it can't change the owner of other files.
- Updates download into a private, freshly made folder, only from GitHub over HTTPS, with a size limit.
- macOS: "Restart with administrator rights" works when the app sits in a folder whose name contains quotes or backslashes. Linux: the desktop entry works for install paths with special characters, and `pkexec` uses `/usr/bin/env` directly.
- Creating a disk image can't overwrite the image it copies from, even when the path is written differently.
- Previews from a previous scan no longer show up for files in a newly opened one, and a damaged image can't stop previews from working; very large images are refused instead of using lots of memory.
- Opening a damaged or oversized scan file can't use up all memory.
- "Resume the previous scan?" keeps your choice to scan without locking or to unmount first.
- "Save scan…" reports success only after the file is written, and shows an error if saving fails.

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
