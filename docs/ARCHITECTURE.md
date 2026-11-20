# Fast File Recovery: architecture

How a scan and a recovery work, and where each piece lives. What the app does for users is in the
[README](../README.md).

## Overview

A Cargo workspace with three crates:

| Crate | Role | Rule |
|---|---|---|
| `crates/ffr-core` | The engine: devices, partitions, file systems, signatures, carving, scanning, recovery, imaging | No GUI, no network, no install code. The app talks to it only through `start_scan`, `start_recovery`, `start_imaging` and the read-only device API |
| `crates/ffr-app` | The egui app: screens, widgets, settings, elevation, install and update | Never writes to the disk being recovered |
| `crates/xtask` | `cargo xtask …`: version bumps, icons and installer pictures, the test sample files | Build tooling in Rust (no npm, no shell scripts) |

**The disk being scanned is only ever opened read-only**, and recovery refuses a destination on that same disk.

## Devices (`ffr-core/src/device/`)

`mod.rs` holds the shared types and the `Platform` trait; each OS implements it:

| OS | Access |
|---|---|
| Windows | `\\.\PhysicalDriveN`, storage IOCTLs for size and model, volume locking |
| Linux | `/sys/block`, `/proc/self/mountinfo`, `O_DIRECT` reads, `BLKROSET` to make the device read-only |
| macOS | `diskutil -plist`, `/dev/rdiskN` with `F_NOCACHE`, unmounting as the lock |
| other | Only disk image files |

`io/` provides aligned buffers, a fixed buffer pool and reads that retry and step around bad sectors.
`image.rs` makes dd-style disk images (`<name>.img` plus `<name>.img.ffrmap` listing unreadable ranges), so a
failing disk can be read once and scanned from the image.

## Two kinds of scan

**Quick scan (`fs/`)** reads the file system's own records of deleted files:

| File system | What it reads |
|---|---|
| NTFS (`ntfs.rs`) | `$MFT` records with the in-use flag clear: names and parent folders from `$FILE_NAME`, data from `$DATA` runs (exact extents, fragmented files too) or resident bytes; `$Bitmap` says whether clusters were reused |
| FAT12/16/32 (`fat.rs`) | Deleted (`0xE5`) directory entries with long names; the FAT chain is cleared on delete, so data is assumed contiguous |
| exFAT (`exfat.rs`) | Directory entry sets with `InUse = 0` |

`partition.rs` reads MBR and GPT tables and detects file systems from boot sectors. `fs/mod.rs`'s `QuickScanner`
is the extension point for more file systems.

**Deep scan (carving)** finds files by their content, whatever the file system says:

```text
reader thread(s) ──chunks──▶ match + resolve workers (rayon, one per core) ──results──▶ aggregator
       ▲                                         │                                        │
       └────────────── buffers recycled ◀────────┘                         events (≤ 10 Hz)
```

- Readers fill buffers from the fixed pool; backpressure keeps memory bounded.
- Workers run the signature matcher over each chunk plus a small overlap, and resolve each candidate's size.
- The aggregator puts results back in disk order, removes duplicates, tracks progress and writes checkpoints
  (`scan/session.rs`: postcard + zstd), so a scan can be saved and reopened.

## Signatures and carving

- `signatures/` is **data**: a registry of magic bytes per format. Adding a format means one entry there, one
  resolver in `carve/`, and one test.
- `carve/` holds the size resolvers and validators: given a hit, find where the file ends (JPEG markers, PNG
  chunks, ISO-BMFF boxes, ZIP central directories, OLE sectors, MP3 frames…). **All disk bytes are hostile**:
  every resolver is bounds-checked, reads through a bounded `Reader` (`carve/reader.rs`, `util.rs`), and gives up
  rather than guessing.
- `kinds.rs` maps every result to its extension and category; hits store a compact `u16` index.

The README's *Supported file types* lists every format.

## Recovery (`ffr-core/src/recover/`)

Destination checks (not the source disk, enough free space), naming and folder layouts (by type, by original
path), a pool of writer threads, and a manifest of what was recovered and from where.

## The app (`crates/ffr-app`)

| Path | Role |
|---|---|
| `main.rs` | CLI flags, self-install dispatch, elevation, the GUI |
| `app.rs` | Global state, the screen router, the sidebar shell, dialogs, job polling |
| `screens/` | Source → Scan → Results → Recover, plus Settings and About |
| `widgets/` | Disk cards, sector map, progress ring, sparkline, hex view, chips, toasts, icons |
| `preview.rs` | Thumbnails decoded off-thread (LRU of 200), text and hex of the first 4 KiB |
| `elevate.rs` | Admin / root per OS: a manifest on Windows, `pkexec` on Linux, `osascript` on macOS |
| `settings.rs` | `settings.toml`, saved on a worker thread |
| `updater.rs` | GitHub releases, SHA-256 verified; never during a scan or recovery, never as root, never in local builds, never without the user clicking Update |
| `install/` | Inno Setup on Windows (`installer/ffr.iss`); self-install on Linux / macOS |
| `theme.rs` | The design system: every colour lives here |

## Tests and benchmarks

`cargo test --workspace` runs the engine against small sample files in `crates/ffr-core/tests/samples`, all
generated from scratch by `cargo xtask samples` (freely licensed, each under 50 KiB). `benches/match_stage.rs`
measures the match stage on 256 MiB of random data (target: ≥ 3 GB/s across cores in release).
