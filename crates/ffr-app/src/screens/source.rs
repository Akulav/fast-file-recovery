//! Source screen: disk cards with partition bars, images, scan mode, presets and locking.

use crate::app::FfrApp;
use crate::theme;
use crate::widgets::{self, disk_card::disk_card, icons::Icon};
use eframe::egui::{self, RichText, Ui};
use ffr_core::device::DeviceKind;
use ffr_core::scan::ScanMode;
use ffr_core::signatures::Preset;

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let ctx_line = match app.selected_device() {
        Some(_) => app.source_title(),
        None => "Pick the disk or image that held the deleted files".into(),
    };
    let can_start = app.selected_device().is_some() && !app.job_running();
    let mut start = false;
    widgets::top_bar(ui, &p, "Choose a source", &ctx_line, |ui| {
        start =
            widgets::primary_button(ui, &p, Some(Icon::Play), "Start scan", can_start).clicked();
    });
    if start {
        app.request_scan(false, false);
    }

    if !app.is_admin {
        let text = if cfg!(windows) {
            "Running without administrator rights: physical disks can't be read. Disk image files still work."
        } else {
            "Running without administrator rights: only disk image files can be scanned."
        };
        ui.horizontal(|ui| {
            widgets::banner(ui, &p, p.warning, Icon::Shield, text);
        });
        if widgets::secondary_button(
            ui,
            &p,
            Some(Icon::Shield),
            "Restart with administrator rights",
            true,
        )
        .clicked()
        {
            match crate::elevate::relaunch_elevated() {
                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => app.toasts.error(e),
            }
        }
        ui.add_space(theme::SPACE);
    }

    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, &p, Some(Icon::Disc), "Open disk image…", true).clicked() {
            app.open_image();
        }
        let can_image = app.selected_device().is_some_and(|d| d.kind != DeviceKind::ImageFile)
            && !app.job_running();
        if widgets::secondary_button(ui, &p, Some(Icon::Copy), "Create disk image…", can_image)
            .on_hover_text("Copy the whole disk to an image file on another disk, then scan the copy. Best for failing drives.")
            .clicked()
        {
            app.create_image();
        }
        if widgets::secondary_button(ui, &p, Some(Icon::Folder), "Open saved scan…", true).clicked() {
            app.open_saved_scan();
        }
        let refreshing = app.devices.refreshing();
        if widgets::secondary_button(ui, &p, Some(Icon::Refresh), if refreshing { "Refreshing…" } else { "Refresh" }, !refreshing)
            .on_hover_text("F5")
            .clicked()
        {
            app.devices.refresh();
        }
    });
    ui.add_space(theme::SPACE * 1.5);

    // Disk grid
    let avail = ui.available_height() - 150.0;
    egui::ScrollArea::vertical()
        .id_salt("disks")
        .max_height(avail.max(160.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let devices: Vec<_> = app
                .devices
                .list
                .iter()
                .chain(app.devices.images.iter())
                .cloned()
                .collect();
            if devices.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    widgets::icons::icon(ui, Icon::HardDrive, 40.0, p.muted);
                    ui.label(widgets::muted(
                        &p,
                        if app.devices.loaded {
                            "No disks found. Open a disk image, or restart with administrator rights."
                        } else {
                            "Looking for disks…"
                        },
                    ));
                });
                return;
            }
            let width = ui.available_width();
            let cols = ((width + 12.0) / 340.0).floor().max(1.0) as usize;
            let card_w = (width - 12.0 * (cols as f32 - 1.0)) / cols as f32;
            for row in devices.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for d in row {
                        let (sel, sel_part) = match &app.selected {
                            Some((id, part)) if *id == d.id => (true, *part),
                            _ => (false, None),
                        };
                        let r = disk_card(ui, &p, d, sel, sel_part, card_w);
                        if r.clicked && !app.job_running() {
                            let part = if sel && r.partition.is_some() && r.partition == sel_part {
                                None
                            } else {
                                r.partition
                            };
                            app.selected = Some((d.id.clone(), part));
                        }
                    }
                });
                ui.add_space(12.0);
            }
        });

    // Scan options
    ui.add_space(theme::SPACE);
    if let Some(dev) = app.selected_device().cloned() {
        if dev.is_system {
            let os = if cfg!(windows) {
                "Windows"
            } else if cfg!(target_os = "macos") {
                "macOS"
            } else {
                "The system"
            };
            widgets::banner(
                ui,
                &p,
                p.warning,
                Icon::Alert,
                &format!("{os} is running from this disk; new writes may overwrite deleted data. Stop using the computer and scan, or make an image from another computer."),
            );
            if cfg!(target_os = "macos") {
                ui.label(widgets::small_muted(
                    &p,
                    "Internal sealed system volumes on Apple Silicon can't be read raw. External disks and images work.",
                ));
            }
            ui.add_space(theme::SPACE);
        }
    }
    widgets::card(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(widgets::small_muted(&p, "Scan mode"));
                widgets::segmented(
                    ui,
                    &p,
                    &mut app.scan_mode,
                    &[
                        (ScanMode::Quick, "Quick"),
                        (ScanMode::Deep, "Deep"),
                        (ScanMode::Full, "Full"),
                    ],
                );
                let hint = match app.scan_mode {
                    ScanMode::Quick => "Reads file system records. Keeps names and folders. Seconds to minutes.",
                    ScanMode::Deep => "Searches the whole disk for file contents. Works on formatted and damaged disks.",
                    ScanMode::Full => "Quick, then Deep. Recommended.",
                };
                ui.label(widgets::small_muted(&p, hint));
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.label(widgets::small_muted(&p, "File types"));
                let mut chosen = None;
                egui::ComboBox::from_id_salt("preset")
                    .selected_text(app.preset_label.clone())
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for preset in Preset::ALL {
                            if ui.selectable_label(app.preset_label == preset.label(), preset.label()).clicked() {
                                chosen = Some(preset);
                            }
                        }
                        ui.separator();
                        ui.label(widgets::small_muted(&p, "Pick single types in Settings → Formats"));
                    });
                if let Some(preset) = chosen {
                    app.settings.enabled_formats = preset.ids();
                    app.preset_label = preset.label().into();
                    app.settings.save_async();
                }
                ui.label(widgets::small_muted(
                    &p,
                    format!("{} formats on", app.settings.enabled_formats.len()),
                ));
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.label(widgets::small_muted(&p, "Lock disk"));
                ui.horizontal(|ui| {
                    widgets::toggle(ui, &p, &mut app.lock_disk).on_hover_text(
                        "Locking stops the system from writing to the disk while it is scanned, so deleted data can't be overwritten. Recommended.",
                    );
                    widgets::icons::icon(
                        ui,
                        if app.lock_disk { Icon::Lock } else { Icon::Unlock },
                        15.0,
                        if app.lock_disk { p.success } else { p.muted },
                    );
                    ui.label(RichText::new(if app.lock_disk { "On" } else { "Off" }).color(p.text));
                });
                ui.label(widgets::small_muted(&p, "Nothing is ever written to the source."));
            });
        });
    });
}
