//! `eframe::App`: global state, screen router, sidebar shell, dialogs and job polling.

use crate::branding::{self, MADE_BY, VERSION};
use crate::install::StartupOffer;
use crate::preview::PreviewManager;
use crate::screens;
use crate::settings::{Settings, ThemePref, WindowGeom};
use crate::theme::{self, Palette};
use crate::updater::{UpdateState, Updater};
use crate::widgets::{self, icons::Icon, toast::Toasts};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{
    self, Align, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2,
};
use ffr_core::carve::Confidence;
use ffr_core::device::{DeviceId, DeviceInfo, DeviceKind};
use ffr_core::error::ScanError;
use ffr_core::image::ImageEvent;
use ffr_core::kinds::Category;
use ffr_core::recover::{DestCheck, RecoverEvent, RecoverSummary};
use ffr_core::scan::{
    Phase, ScanEvent, ScanHandle, ScanMode, ScanOptions, Session, SourceSpec, Summary,
};
use ffr_core::JobHandle;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Source,
    Scan,
    Results,
    Recover,
    Settings,
    About,
}

pub enum Dialog {
    LockFailed(Vec<String>),
    ConfirmUnmount(Vec<String>),
    Install(StartupOffer),
    Resume(Box<Session>),
    Imaging,
}

/// Device list, refreshed on a worker thread.
#[derive(Default)]
pub struct Devices {
    pub list: Vec<DeviceInfo>,
    pub images: Vec<DeviceInfo>,
    rx: Option<Receiver<Vec<DeviceInfo>>>,
    last: Option<Instant>,
    pub loaded: bool,
}

impl Devices {
    pub fn refresh(&mut self) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        self.last = Some(Instant::now());
        std::thread::spawn(move || {
            let _ = tx.send(ffr_core::device::list_devices());
        });
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            if let Ok(v) = rx.try_recv() {
                self.list = v;
                self.loaded = true;
                self.rx = None;
            }
        }
    }

    pub fn refreshing(&self) -> bool {
        self.rx.is_some()
    }

    pub fn find(&self, id: &DeviceId) -> Option<&DeviceInfo> {
        self.list
            .iter()
            .chain(self.images.iter())
            .find(|d| &d.id == id)
    }
}

pub struct ScanState {
    pub handle: ScanHandle,
    rx: Receiver<ScanEvent>,
    pub phase: Phase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bps: f64,
    pub eta: Option<Duration>,
    pub started: Instant,
    pub finished_at: Option<Instant>,
    pub paused_total: Duration,
    pub paused_since: Option<Instant>,
    pub throughput: VecDeque<f32>,
    last_sample: Instant,
    pub sector_map: Vec<u8>,
    pub warnings: Vec<String>,
    pub done: Option<Summary>,
    pub failed: Option<ScanError>,
    pub lock_desc: Option<String>,
    pub bad_ranges: usize,
    pub smooth: f32,
    pub title: String,
    pub opts: ScanOptions,
}

impl ScanState {
    pub fn running(&self) -> bool {
        self.done.is_none() && self.failed.is_none()
    }

    pub fn elapsed(&self) -> Duration {
        let end = self.finished_at.unwrap_or_else(Instant::now);
        let paused = self.paused_total + self.paused_since.map(|t| t.elapsed()).unwrap_or_default();
        end.duration_since(self.started).saturating_sub(paused)
    }

    pub fn fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            0.0
        } else {
            self.bytes_done as f32 / self.bytes_total as f32
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortCol {
    Name,
    Ext,
    Size,
    Confidence,
    Offset,
    Path,
}

pub struct ResultsState {
    pub cat_on: [bool; 6],
    pub ext_off: HashSet<String>,
    pub min_size: u64,
    pub max_size: u64,
    pub min_conf: Confidence,
    pub named_only: bool,
    pub search: String,
    pub sort: SortCol,
    pub asc: bool,
    pub filtered: Vec<u32>,
    pub dirty: bool,
    pub last_build: Instant,
    pub selected: Vec<bool>,
    pub sel_count: usize,
    pub sel_bytes: u64,
    pub anchor: Option<usize>,
    pub focus: Option<u32>,
    /// (ext, category, count, bytes) over all hits.
    pub ext_counts: Vec<(String, Category, usize, u64)>,
    pub cat_counts: [(usize, u64); 6],
    pub focus_search: bool,
    pub scroll_to: Option<usize>,
}

impl Default for ResultsState {
    fn default() -> Self {
        Self {
            cat_on: [true; 6],
            ext_off: HashSet::new(),
            min_size: 0,
            max_size: 0,
            min_conf: Confidence::Low,
            named_only: false,
            search: String::new(),
            sort: SortCol::Offset,
            asc: true,
            filtered: Vec::new(),
            dirty: true,
            last_build: Instant::now() - Duration::from_secs(10),
            selected: Vec::new(),
            sel_count: 0,
            sel_bytes: 0,
            anchor: None,
            focus: None,
            ext_counts: Vec::new(),
            cat_counts: [(0, 0); 6],
            focus_search: false,
            scroll_to: None,
        }
    }
}

impl ResultsState {
    pub fn set_selected(&mut self, session: &Session, id: u32, on: bool) {
        let i = id as usize;
        if self.selected.len() < session.hits.len() {
            self.selected.resize(session.hits.len(), false);
        }
        if self.selected[i] != on {
            self.selected[i] = on;
            let len = session.hits[i].len;
            if on {
                self.sel_count += 1;
                self.sel_bytes += len;
            } else {
                self.sel_count -= 1;
                self.sel_bytes -= len;
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.iter_mut().for_each(|s| *s = false);
        self.sel_count = 0;
        self.sel_bytes = 0;
    }

    pub fn selection(&self) -> Vec<u32> {
        self.selected
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| i as u32)
            .collect()
    }
}

pub struct RecoverState {
    pub dest: Option<PathBuf>,
    pub check: Option<DestCheck>,
    pub dest_rotational: bool,
    check_rx: Option<Receiver<(DestCheck, bool)>>,
    pub job: Option<(JobHandle, Receiver<RecoverEvent>)>,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bps: f64,
    pub current: String,
    pub summary: Option<RecoverSummary>,
    pub check_key: Option<(PathBuf, usize, u64)>,
}

impl Default for RecoverState {
    fn default() -> Self {
        Self {
            dest: None,
            check: None,
            dest_rotational: false,
            check_rx: None,
            job: None,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            bps: 0.0,
            current: String::new(),
            summary: None,
            check_key: None,
        }
    }
}

impl RecoverState {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Re-run destination checks on a worker when the destination or selection changes.
    pub fn ensure_check(
        &mut self,
        source: &DeviceInfo,
        devices: &[DeviceInfo],
        count: usize,
        bytes: u64,
    ) {
        let Some(dest) = self.dest.clone() else {
            return;
        };
        let key = (dest.clone(), count, bytes);
        if self.check_key.as_ref() == Some(&key) {
            return;
        }
        self.check_key = Some(key);
        self.check = None;
        let (tx, rx) = unbounded();
        self.check_rx = Some(rx);
        let source = source.clone();
        let devices = devices.to_vec();
        std::thread::spawn(move || {
            let check = ffr_core::recover::check_destination(&source, &dest, bytes);
            let disks = ffr_core::device::disks_of_path(&dest);
            let rotational = devices
                .iter()
                .any(|d| disks.contains(&d.id) && d.rotational == Some(true));
            let _ = tx.send((check, rotational));
        });
    }

    fn poll(&mut self) -> Vec<RecoverEvent> {
        if let Some(rx) = &self.check_rx {
            if let Ok((c, rot)) = rx.try_recv() {
                self.check = Some(c);
                self.dest_rotational = rot;
                self.check_rx = None;
            }
        }
        let mut out = Vec::new();
        if let Some((_, rx)) = &self.job {
            while let Ok(e) = rx.try_recv() {
                out.push(e);
            }
        }
        out
    }
}

pub struct ImagingState {
    pub handle: JobHandle,
    rx: Receiver<ImageEvent>,
    pub dest: PathBuf,
    pub done: u64,
    pub total: u64,
    pub bps: f64,
    pub bad: u64,
    pub finished: Option<Result<PathBuf, String>>,
}

pub struct FfrApp {
    pub p: Palette,
    pub settings: Settings,
    dark_applied: Option<bool>,
    pub screen: Screen,
    pub toasts: Toasts,
    pub devices: Devices,
    pub selected: Option<(DeviceId, Option<u32>)>,
    pub scan_mode: ScanMode,
    pub preset_label: String,
    pub lock_disk: bool,
    pub scan: Option<ScanState>,
    pub session: Option<Session>,
    pub results: ResultsState,
    pub recover: RecoverState,
    pub imaging: Option<ImagingState>,
    pub updater: Updater,
    pub preview: PreviewManager,
    pub dialog: Option<Dialog>,
    pub is_admin: bool,
    pending_start: Option<(Receiver<Option<Session>>, bool, bool)>,
    session_io: Option<Receiver<Result<Session, String>>>,
    last_frame: Instant,
    geom_changed: Option<Instant>,
    pub category_totals: [(u64, u64); 6],
}

impl FfrApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        startup: StartupOffer,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::install_fonts(&cc.egui_ctx);
        crate::platform_window::set_window_icons(cc);
        let mut app = Self {
            p: Palette::dark(),
            scan_mode: settings.default_mode,
            settings,
            dark_applied: None,
            screen: Screen::Source,
            toasts: Toasts::default(),
            devices: Devices::default(),
            selected: None,
            preset_label: "Custom".into(),
            lock_disk: true,
            scan: None,
            session: None,
            results: ResultsState::default(),
            recover: RecoverState::default(),
            imaging: None,
            updater: Updater::default(),
            preview: PreviewManager::new(),
            dialog: None,
            is_admin: crate::elevate::is_elevated(),
            pending_start: None,
            session_io: None,
            last_frame: Instant::now(),
            geom_changed: None,
            category_totals: [(0, 0); 6],
        };
        app.preset_label = screens::settings::preset_name(&app.settings.enabled_formats);
        if startup != StartupOffer::None {
            app.dialog = Some(Dialog::Install(startup));
        }
        app.devices.refresh();
        app
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => ctx
                .system_theme()
                .map(|t| t == egui::Theme::Dark)
                .unwrap_or(true),
        };
        if self.dark_applied != Some(dark) {
            self.p = if dark {
                Palette::dark()
            } else {
                Palette::light()
            };
            theme::apply(ctx, &self.p);
            self.dark_applied = Some(dark);
        }
    }

    pub fn job_running(&self) -> bool {
        self.scan.as_ref().is_some_and(|s| s.running())
            || self.recover.running()
            || self.imaging.as_ref().is_some_and(|i| i.finished.is_none())
    }

    pub fn selected_device(&self) -> Option<&DeviceInfo> {
        let (id, _) = self.selected.as_ref()?;
        self.devices.find(id)
    }

    pub fn source_title(&self) -> String {
        let Some(dev) = self.selected_device() else {
            return "No disk selected".into();
        };
        let mut t = dev.title();
        if let Some((_, Some(pi))) = &self.selected {
            if let Some(part) = dev.partitions.iter().find(|p| p.index == *pi) {
                t.push_str(&format!(
                    " · partition {} ({})",
                    part.index,
                    part.fs.label()
                ));
            }
        }
        t
    }

    // ------------------------------------------------------------ scanning

    /// Begin the start flow: look for a resumable checkpoint first.
    pub fn request_scan(&mut self, unlocked: bool, unmount_confirmed: bool) {
        let Some(dev) = self.selected_device().cloned() else {
            self.toasts.warning("Pick a disk or image first.");
            return;
        };
        if !dev.readable && dev.kind != DeviceKind::ImageFile {
            self.toasts.error(
                "This disk can't be read without administrator rights. Restart with administrator rights first.",
            );
            return;
        }
        // Linux/macOS: locking unmounts partitions; ask first.
        let mounted: Vec<String> = dev
            .partitions
            .iter()
            .flat_map(|p| p.mount_points.iter().cloned())
            .collect();
        let needs_unmount = cfg!(any(target_os = "linux", target_os = "macos"))
            && self.lock_disk
            && !unlocked
            && !dev.is_system
            && dev.kind == DeviceKind::PhysicalDisk
            && !mounted.is_empty();
        if needs_unmount && !unmount_confirmed {
            self.dialog = Some(Dialog::ConfirmUnmount(mounted));
            return;
        }
        let range = self.selected_range(&dev);
        let (tx, rx) = unbounded();
        self.pending_start = Some((rx, unlocked, needs_unmount));
        let dir = crate::paths::sessions_dir();
        std::thread::spawn(move || {
            let _ = tx.send(find_resumable(&dir, &dev.id, &range));
        });
    }

    fn selected_range(&self, dev: &DeviceInfo) -> Option<std::ops::Range<u64>> {
        let (_, pi) = self.selected.as_ref()?;
        let pi = (*pi)?;
        dev.partitions
            .iter()
            .find(|p| p.index == pi)
            .map(|p| p.offset..p.offset + p.len)
    }

    pub fn start_scan_now(&mut self, resume: Option<Session>, unlocked: bool, unmount: bool) {
        let Some(dev) = self.selected_device().cloned() else {
            return;
        };
        let range = self.selected_range(&dev);
        let source = SourceSpec {
            device: dev.clone(),
            range,
        };
        let s = &self.settings;
        let mut opts = ScanOptions {
            mode: self.scan_mode,
            enabled: s.enabled_formats.clone(),
            chunk_size: s.chunk_mib as usize * 1024 * 1024,
            queue_depth: s.queue_depth as usize,
            retries: s.retries,
            cluster_aligned_only: s.cluster_aligned_only,
            lock: self.lock_disk && !unlocked,
            unmount,
            checkpoint_dir: Some(crate::paths::sessions_dir()),
            ..Default::default()
        };
        let mut session = Session::new(opts.session_id.clone(), source.clone(), self.scan_mode);
        if let Some(r) = resume {
            opts.session_id = r.id.clone();
            session = Session::new(r.id.clone(), source.clone(), r.mode);
            opts.mode = r.mode;
            opts.resume = Some(Box::new(r));
        }
        self.preview.clear();
        self.results = ResultsState::default();
        self.recover = RecoverState::default();
        self.category_totals = [(0, 0); 6];
        let (handle, rx) = ffr_core::start_scan(source.clone(), opts.clone());
        self.session = Some(session);
        self.scan = Some(ScanState {
            handle,
            rx,
            phase: Phase::Preparing,
            bytes_done: 0,
            bytes_total: 0,
            bps: 0.0,
            eta: None,
            started: Instant::now(),
            finished_at: None,
            paused_total: Duration::ZERO,
            paused_since: None,
            throughput: VecDeque::new(),
            last_sample: Instant::now(),
            sector_map: vec![0; ffr_core::scan::SECTOR_MAP_CELLS],
            warnings: Vec::new(),
            done: None,
            failed: None,
            lock_desc: None,
            bad_ranges: 0,
            smooth: 0.0,
            title: self.source_title(),
            opts: ScanOptions {
                resume: None,
                ..opts
            },
        });
        self.screen = Screen::Scan;
    }

    fn poll_scan(&mut self) {
        let Some(scan) = &mut self.scan else { return };
        let mut new_hits = false;
        let mut events = Vec::new();
        while let Ok(e) = scan.rx.try_recv() {
            events.push(e);
        }
        for e in events {
            match e {
                ScanEvent::Phase(p) => scan.phase = p,
                ScanEvent::Locked(d) => scan.lock_desc = Some(d),
                ScanEvent::Progress {
                    bytes_done,
                    bytes_total,
                    bytes_per_sec,
                    eta,
                } => {
                    scan.bytes_done = bytes_done;
                    scan.bytes_total = bytes_total;
                    scan.bps = bytes_per_sec;
                    scan.eta = eta;
                }
                ScanEvent::Hits(batch) => {
                    for h in &batch.hits {
                        let c = h.kind().category.index();
                        self.category_totals[c].0 += 1;
                        self.category_totals[c].1 += h.len;
                    }
                    if let Some(s) = &mut self.session {
                        s.apply(batch);
                    }
                    new_hits = true;
                }
                ScanEvent::BadRange(_) => scan.bad_ranges += 1,
                ScanEvent::SectorMap(m) => scan.sector_map = m,
                ScanEvent::Warning(w) => {
                    self.toasts.warning(w.clone());
                    scan.warnings.push(w);
                }
                ScanEvent::Done(summary) => {
                    scan.finished_at = Some(Instant::now());
                    if summary.cancelled {
                        self.toasts
                            .info("Scan stopped. It was saved, so you can resume it later.");
                    } else {
                        self.toasts
                            .success(format!("Scan finished: {} files found.", summary.hits));
                    }
                    if let Some(s) = &mut self.session {
                        s.finished = !summary.cancelled;
                    }
                    scan.done = Some(summary);
                    new_hits = true;
                }
                ScanEvent::Failed(err) => {
                    scan.finished_at = Some(Instant::now());
                    match &err {
                        ScanError::LockFailed(v) => {
                            self.dialog = Some(Dialog::LockFailed(v.clone()))
                        }
                        e => self.toasts.error(e.to_string()),
                    }
                    scan.failed = Some(err);
                }
            }
        }
        if scan.last_sample.elapsed() >= Duration::from_secs(1) && scan.running() {
            scan.last_sample = Instant::now();
            if !scan.handle.is_paused() {
                scan.throughput.push_back(scan.bps as f32);
                while scan.throughput.len() > 60 {
                    scan.throughput.pop_front();
                }
            }
        }
        if new_hits {
            self.results.dirty = true;
        }
    }

    pub fn toggle_pause(&mut self) {
        if let Some(s) = &mut self.scan {
            if s.handle.is_paused() {
                s.handle.resume();
                if let Some(t) = s.paused_since.take() {
                    s.paused_total += t.elapsed();
                }
            } else {
                s.handle.pause();
                s.paused_since = Some(Instant::now());
            }
        }
    }

    pub fn stop_scan(&mut self) {
        if let Some(s) = &mut self.scan {
            s.handle.cancel();
            if let Some(t) = s.paused_since.take() {
                s.paused_total += t.elapsed();
            }
        }
    }

    // ------------------------------------------------------------ sessions

    pub fn open_saved_scan(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open a saved scan")
            .add_filter("Fast File Recovery scan", &["ffrscan"])
            .set_directory(crate::paths::sessions_dir())
            .pick_file()
        else {
            return;
        };
        let (tx, rx) = unbounded();
        self.session_io = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(ffr_core::scan::load_session(&path).map_err(|e| e.to_string()));
        });
    }

    pub fn save_scan_as(&mut self) {
        let Some(s) = self.session.clone() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save scan")
            .add_filter("Fast File Recovery scan", &["ffrscan"])
            .set_file_name(format!("{}.ffrscan", s.id))
            .save_file()
        else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = ffr_core::scan::save_session(&path, &s) {
                tracing::warn!("save failed: {e}");
            }
            crate::elevate::give_to_original_user(&path);
        });
        self.toasts.success("Scan saved.");
    }

    fn poll_sessions(&mut self) {
        if let Some((rx, unlocked, unmount)) = &self.pending_start {
            if let Ok(found) = rx.try_recv() {
                let (unlocked, unmount) = (*unlocked, *unmount);
                self.pending_start = None;
                match found {
                    Some(s) => self.dialog = Some(Dialog::Resume(Box::new(s))),
                    None => self.start_scan_now(None, unlocked, unmount),
                }
            }
        }
        if let Some(rx) = &self.session_io {
            if let Ok(r) = rx.try_recv() {
                self.session_io = None;
                match r {
                    Ok(s) => {
                        self.preview.clear();
                        self.results = ResultsState::default();
                        self.recover = RecoverState::default();
                        self.category_totals = [(0, 0); 6];
                        for h in &s.hits {
                            let c = h.kind().category.index();
                            self.category_totals[c].0 += 1;
                            self.category_totals[c].1 += h.len;
                        }
                        if !self.devices.list.iter().any(|d| d.id == s.source.device.id)
                            && s.source.device.kind == DeviceKind::ImageFile
                        {
                            self.devices.images.push(s.source.device.clone());
                        }
                        self.selected = Some((s.source.device.id.clone(), None));
                        self.toasts
                            .success(format!("Opened a scan with {} files.", s.hits.len()));
                        self.session = Some(s);
                        self.scan = None;
                        self.screen = Screen::Results;
                    }
                    Err(e) => self.toasts.error(e),
                }
            }
        }
    }

    // ------------------------------------------------------------ images

    pub fn open_image(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open disk image")
            .add_filter(
                "Disk images",
                &["img", "dd", "raw", "bin", "iso", "dmg", "vhd"],
            )
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        match ffr_core::device::image_device(&path) {
            Ok(d) => {
                self.selected = Some((d.id.clone(), None));
                if !self.devices.images.iter().any(|x| x.id == d.id) {
                    self.devices.images.push(d);
                }
            }
            Err(e) => self.toasts.error(e.to_string()),
        }
    }

    pub fn create_image(&mut self) {
        let Some(dev) = self.selected_device().cloned() else {
            self.toasts.warning("Pick the disk to copy first.");
            return;
        };
        let name: String = dev
            .model
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let Some(dest) = rfd::FileDialog::new()
            .set_title("Save disk image on another disk")
            .add_filter("Disk image", &["img"])
            .set_file_name(format!("{name}.img"))
            .save_file()
        else {
            return;
        };
        let range = self.selected_range(&dev);
        let (handle, rx) = ffr_core::start_imaging(
            SourceSpec { device: dev, range },
            dest.clone(),
            ffr_core::ImageOptions {
                retries: self.settings.retries,
                ..Default::default()
            },
        );
        self.imaging = Some(ImagingState {
            handle,
            rx,
            dest,
            done: 0,
            total: 0,
            bps: 0.0,
            bad: 0,
            finished: None,
        });
        self.dialog = Some(Dialog::Imaging);
    }

    fn poll_imaging(&mut self) {
        let Some(img) = &mut self.imaging else { return };
        while let Ok(e) = img.rx.try_recv() {
            match e {
                ImageEvent::Progress {
                    bytes_done,
                    bytes_total,
                    bytes_per_sec,
                    bad_bytes,
                } => {
                    img.done = bytes_done;
                    img.total = bytes_total;
                    img.bps = bytes_per_sec;
                    img.bad = bad_bytes;
                }
                ImageEvent::Done {
                    path,
                    cancelled,
                    bad_ranges,
                    ..
                } => {
                    img.done = img.total;
                    crate::elevate::give_to_original_user(&path);
                    if cancelled {
                        img.finished = Some(Err("Imaging was cancelled.".into()));
                    } else {
                        if bad_ranges > 0 {
                            self.toasts.warning(format!(
                                "Image created with {bad_ranges} unreadable areas (zero-filled)."
                            ));
                        }
                        img.finished = Some(Ok(path));
                    }
                }
                ImageEvent::Failed(e) => img.finished = Some(Err(e)),
            }
        }
    }

    // ------------------------------------------------------------ recovery

    fn poll_recover(&mut self) {
        for e in self.recover.poll() {
            match e {
                RecoverEvent::Progress {
                    files_done,
                    files_total,
                    bytes_done,
                    bytes_total,
                    bytes_per_sec,
                    current,
                } => {
                    let r = &mut self.recover;
                    r.files_done = files_done;
                    r.files_total = files_total;
                    r.bytes_done = bytes_done;
                    r.bytes_total = bytes_total;
                    r.bps = bytes_per_sec;
                    r.current = current;
                }
                RecoverEvent::Done(s) => {
                    self.recover.job = None;
                    crate::elevate::give_to_original_user(&s.folder);
                    if s.failed > 0 {
                        self.toasts.warning(format!(
                            "Recovered {} files; {} failed.",
                            s.recovered + s.partial,
                            s.failed
                        ));
                    } else {
                        self.toasts
                            .success(format!("Recovered {} files.", s.recovered + s.partial));
                    }
                    self.recover.summary = Some(s);
                }
                RecoverEvent::Failed(e) => {
                    self.recover.job = None;
                    self.toasts.error(e);
                }
            }
        }
    }

    // ------------------------------------------------------------ shell

    fn sidebar(&mut self, ctx: &egui::Context) {
        let p = self.p;
        egui::SidePanel::left("sidebar")
            .exact_width(theme::SIDEBAR_WIDTH)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(Margin::same(14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_bytes("bytes://ffr-icon-256.png", branding::ICON_256)
                            .fit_to_exact_size(Vec2::splat(34.0)),
                    );
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(
                            RichText::new("Fast File")
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new("Recovery")
                                .font(theme::semibold(14.0))
                                .color(p.accent),
                        );
                    });
                });
                ui.add_space(18.0);
                let has_scan = self.scan.is_some() || self.session.is_some();
                let has_hits = self.session.as_ref().is_some_and(|s| !s.hits.is_empty());
                let steps = [
                    (Screen::Source, "Source", Icon::HardDrive, true, has_scan),
                    (
                        Screen::Scan,
                        "Scan",
                        Icon::Scan,
                        self.scan.is_some(),
                        self.scan.as_ref().is_some_and(|s| s.done.is_some()),
                    ),
                    (
                        Screen::Results,
                        "Results",
                        Icon::List,
                        has_hits || self.scan.is_some(),
                        self.results.sel_count > 0,
                    ),
                    (
                        Screen::Recover,
                        "Recover",
                        Icon::Recover,
                        self.results.sel_count > 0 || self.recover.summary.is_some(),
                        self.recover.summary.is_some(),
                    ),
                ];
                for (i, (screen, label, icon, reachable, done)) in steps.into_iter().enumerate() {
                    if self.nav_item(
                        ui,
                        screen,
                        &format!("{}", i + 1),
                        label,
                        icon,
                        reachable,
                        done,
                    ) {
                        self.screen = screen;
                    }
                    if i < 3 {
                        let (r, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 8.0),
                            Sense::hover(),
                        );
                        ui.painter().line_segment(
                            [
                                egui::pos2(r.left() + 19.0, r.top()),
                                egui::pos2(r.left() + 19.0, r.bottom()),
                            ],
                            Stroke::new(1.5_f32, p.border),
                        );
                    }
                }
                ui.add_space(18.0);
                ui.separator();
                ui.add_space(6.0);
                if self.nav_item(
                    ui,
                    Screen::Settings,
                    "",
                    "Settings",
                    Icon::Settings,
                    true,
                    false,
                ) {
                    self.screen = Screen::Settings;
                }
                if self.nav_item(ui, Screen::About, "", "About", Icon::Info, true, false) {
                    self.screen = Screen::About;
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.label(widgets::small_muted(&p, format!("{MADE_BY} · v{VERSION}")));
                    if let Some(r) = self.updater.available().cloned() {
                        ui.add_space(4.0);
                        let text = format!("Update to v{}", r.version);
                        let resp = Frame::new()
                            .fill(Palette::soft(p.accent, 40))
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    widgets::icons::icon(ui, Icon::Download, 14.0, p.accent);
                                    ui.label(RichText::new(text).color(p.accent).size(12.0));
                                });
                            })
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            self.screen = Screen::About;
                        }
                    }
                });
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn nav_item(
        &self,
        ui: &mut egui::Ui,
        screen: Screen,
        number: &str,
        label: &str,
        icon: Icon,
        reachable: bool,
        done: bool,
    ) -> bool {
        let p = &self.p;
        let active = self.screen == screen;
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 38.0),
            if reachable {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let h = ui
            .ctx()
            .animate_bool_with_time(resp.id, resp.hovered() && reachable, theme::ANIM);
        let a = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("a"), active, theme::ANIM);
        let fill = Palette::mix(
            Palette::mix(p.surface, p.raised, h),
            Palette::soft(p.accent, 50),
            a,
        );
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::CONTROL_RADIUS + 2), fill);
        if a > 0.0 {
            let bar =
                egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 9.0), Vec2::new(3.0 * a, 20.0));
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), p.accent);
        }
        let fg = if !reachable {
            Palette::mix(p.muted, p.surface, 0.45)
        } else if active {
            p.text
        } else {
            Palette::mix(p.muted, p.text, h)
        };
        let c = egui::pos2(rect.left() + 19.0, rect.center().y);
        if number.is_empty() {
            widgets::icons::paint(
                ui,
                egui::Rect::from_center_size(c, Vec2::splat(17.0)),
                icon,
                fg,
            );
        } else {
            let circle_fill = if done {
                p.success
            } else if active {
                p.accent
            } else {
                p.raised
            };
            ui.painter().circle_filled(c, 11.0, circle_fill);
            if !done && !active {
                ui.painter()
                    .circle_stroke(c, 11.0, Stroke::new(1.0_f32, p.border));
            }
            if done {
                widgets::icons::paint(
                    ui,
                    egui::Rect::from_center_size(c, Vec2::splat(13.0)),
                    Icon::Check,
                    p.on_accent,
                );
            } else {
                ui.painter().text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    number,
                    theme::semibold(12.0),
                    if active { p.on_accent } else { fg },
                );
            }
        }
        ui.painter().text(
            egui::pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::semibold(13.5),
            fg,
        );
        let resp = if reachable {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            resp.on_hover_text("Not available yet")
        };
        resp.clicked() && reachable
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let p = self.p;
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::new("dialog"))
            .frame(widgets::raised_card(&p).inner_margin(Margin::same(22)));
        let resp = modal.show(ctx, |ui| {
            ui.set_max_width(460.0);
            match &dialog {
                Dialog::LockFailed(vols) => {
                    widgets::section_title(ui, &p, "Couldn't lock the disk");
                    ui.label(widgets::muted(&p, "These volumes are in use by other programs:"));
                    for v in vols {
                        ui.label(widgets::mono(&p, format!("  • {v}")));
                    }
                    ui.label(widgets::muted(&p, "Close programs that use them and retry, or continue without locking (the system may write to the disk)."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, &p, Some(Icon::Refresh), "Retry", true).clicked() {
                            keep = false;
                            self.request_scan(false, true);
                        }
                        if widgets::secondary_button(ui, &p, Some(Icon::Unlock), "Continue unlocked", true).clicked() {
                            keep = false;
                            self.request_scan(true, true);
                        }
                        if widgets::secondary_button(ui, &p, None, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::ConfirmUnmount(mounts) => {
                    widgets::section_title(ui, &p, "Unmount the disk?");
                    ui.label(widgets::muted(&p, "Locking unmounts these so nothing can write to the disk while it is scanned:"));
                    for m in mounts {
                        ui.label(widgets::mono(&p, format!("  • {m}")));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, &p, Some(Icon::Lock), "Unmount and scan", true).clicked() {
                            keep = false;
                            self.request_scan(false, true);
                        }
                        if widgets::secondary_button(ui, &p, Some(Icon::Unlock), "Scan without locking", true).clicked() {
                            keep = false;
                            self.request_scan(true, true);
                        }
                        if widgets::secondary_button(ui, &p, None, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::Install(offer) => {
                    let (title, text, action) = match offer {
                        StartupOffer::UpdateInstalled { installed } => (
                            "Update the installed copy?",
                            format!(
                                "Version {} is installed. This file is version {VERSION}.",
                                installed.clone().unwrap_or_else(|| "?".into())
                            ),
                            "Update installed copy",
                        ),
                        _ => (
                            "Install Fast File Recovery?",
                            "Install it for this user so it appears in your applications and can update itself. Nothing is installed system-wide.".to_string(),
                            "Install for this user",
                        ),
                    };
                    widgets::section_title(ui, &p, title);
                    ui.label(widgets::muted(&p, text));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, &p, Some(Icon::Download), action, true).clicked() {
                            keep = false;
                            match crate::install::install_and_relaunch() {
                                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                Err(e) => self.toasts.error(e),
                            }
                        }
                        if widgets::secondary_button(ui, &p, None, "Just run it", true).clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::Resume(s) => {
                    widgets::section_title(ui, &p, "Resume the previous scan?");
                    let pct = if s.source.device.size > 0 {
                        s.bytes_scanned() as f64 / s.source.device.size as f64 * 100.0
                    } else {
                        0.0
                    };
                    ui.label(widgets::muted(&p, format!(
                        "A scan of this disk stopped at {pct:.0}% with {} files found. Continue where it stopped, or start over.",
                        s.hits.len()
                    )));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, &p, Some(Icon::Play), "Resume", true).clicked() {
                            keep = false;
                            self.start_scan_now(Some((**s).clone()), false, false);
                        }
                        if widgets::secondary_button(ui, &p, Some(Icon::Refresh), "Start over", true).clicked() {
                            keep = false;
                            self.start_scan_now(None, false, false);
                        }
                    });
                }
                Dialog::Imaging => {
                    widgets::section_title(ui, &p, "Creating disk image");
                    if let Some(img) = &self.imaging {
                        ui.label(widgets::muted(&p, img.dest.display().to_string()));
                        let f = if img.total > 0 { img.done as f32 / img.total as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(f).show_percentage().animate(img.finished.is_none()));
                        ui.label(widgets::mono(&p, format!(
                            "{} / {} · {}/s{}",
                            ffr_core::util::human_size(img.done),
                            ffr_core::util::human_size(img.total),
                            ffr_core::util::human_size(img.bps as u64),
                            if img.bad > 0 { format!(" · {} unreadable", ffr_core::util::human_size(img.bad)) } else { String::new() }
                        )));
                        ui.add_space(8.0);
                        match &img.finished {
                            None => {
                                if widgets::danger_button(ui, &p, Some(Icon::Stop), "Cancel", true).clicked() {
                                    img.handle.cancel();
                                }
                            }
                            Some(Ok(path)) => {
                                let path = path.clone();
                                ui.horizontal(|ui| {
                                    if widgets::primary_button(ui, &p, Some(Icon::Scan), "Scan this image", true).clicked() {
                                        keep = false;
                                        match ffr_core::device::image_device(&path) {
                                            Ok(d) => {
                                                self.selected = Some((d.id.clone(), None));
                                                self.devices.images.push(d);
                                                self.screen = Screen::Source;
                                            }
                                            Err(e) => self.toasts.error(e.to_string()),
                                        }
                                    }
                                    if widgets::secondary_button(ui, &p, None, "Close", true).clicked() {
                                        keep = false;
                                    }
                                });
                            }
                            Some(Err(e)) => {
                                widgets::banner(ui, &p, p.danger, Icon::Alert, e);
                                if widgets::secondary_button(ui, &p, None, "Close", true).clicked() {
                                    keep = false;
                                }
                            }
                        }
                    }
                }
            }
        });
        let busy_imaging = matches!(dialog, Dialog::Imaging)
            && self.imaging.as_ref().is_some_and(|i| i.finished.is_none());
        if resp.should_close() && !busy_imaging {
            keep = false;
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        let (open, f5, find, recover) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.key_pressed(egui::Key::F5),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.modifiers.command && i.key_pressed(egui::Key::R),
            )
        });
        if self.dialog.is_some() {
            return;
        }
        if open {
            self.open_image();
        }
        if f5 {
            self.devices.refresh();
        }
        if find && self.session.is_some() {
            self.screen = Screen::Results;
            self.results.focus_search = true;
        }
        if recover && self.results.sel_count > 0 {
            self.screen = Screen::Recover;
        }
    }

    fn remember_window(&mut self, ctx: &egui::Context) {
        let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
        if let (Some(o), Some(i)) = (outer, inner) {
            let g = WindowGeom {
                x: o.min.x,
                y: o.min.y,
                w: i.width(),
                h: i.height(),
            };
            if self.settings.window != Some(g) {
                self.settings.window = Some(g);
                self.geom_changed = Some(Instant::now());
            }
        }
        if self
            .geom_changed
            .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            self.geom_changed = None;
            self.settings.save_async();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.settings.save_now();
            if let Some(s) = &self.scan {
                s.handle.cancel();
            }
        }
    }
}

/// Newest unfinished checkpoint for this device and range.
fn find_resumable(
    dir: &std::path::Path,
    id: &DeviceId,
    range: &Option<std::ops::Range<u64>>,
) -> Option<Session> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "ffrscan"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, p) in files.into_iter().take(10) {
        if let Ok(s) = ffr_core::scan::load_session(&p) {
            if !s.finished
                && &s.source.device.id == id
                && &s.source.range == range
                && !s.scanned.is_empty()
            {
                return Some(s);
            }
        }
    }
    None
}

impl eframe::App for FfrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.2);
        self.last_frame = Instant::now();
        self.apply_theme(ctx);
        self.devices.poll();
        self.poll_scan();
        self.poll_sessions();
        self.poll_recover();
        self.poll_imaging();
        self.preview.poll(ctx);
        self.updater.tick(self.settings.auto_update);
        if self.updater.poll() {
            if self.job_running() {
                self.updater.state =
                    UpdateState::Error("Finish or stop the current job first.".into());
            } else {
                match self.updater.apply() {
                    Ok(()) => {
                        self.settings.save_now();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.toasts.error(e),
                }
            }
        }
        if self.screen == Screen::Source
            && self
                .devices
                .last
                .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
            && !self.job_running()
        {
            self.devices.refresh();
        }
        if let Some(s) = &mut self.scan {
            let target = s.fraction();
            widgets::lerp_towards(&mut s.smooth, target, dt);
        }
        self.keyboard(ctx);
        self.remember_window(ctx);

        self.sidebar(ctx);
        let p = self.p;
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(24, 20)),
            )
            .show(ctx, |ui| match self.screen {
                Screen::Source => screens::source::ui(self, ui),
                Screen::Scan => screens::scan::ui(self, ui),
                Screen::Results => screens::results::ui(self, ui),
                Screen::Recover => screens::recover::ui(self, ui),
                Screen::Settings => screens::settings::ui(self, ui),
                Screen::About => screens::about::ui(self, ui),
            });
        self.dialogs(ctx);
        self.toasts.show(ctx, &p);
        if self.job_running()
            || self.updater.busy()
            || self.devices.refreshing()
            || self.pending_start.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.screen == Screen::Source {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if let Some(s) = &self.scan {
            if (s.smooth - s.fraction()).abs() > 0.001 {
                ctx.request_repaint();
            }
        }
    }
}

/// Open a folder or file in the system file manager.
pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open {}: {e}", path.display()))
}
