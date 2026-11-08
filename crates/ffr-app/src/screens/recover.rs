//! Recover screen: destination, layout, checks, progress and summary.

use super::fmt_duration;
use crate::app::{open_path, FfrApp, Screen};
use crate::theme::{self, Palette};
use crate::widgets::{self, icons::Icon};
use eframe::egui::{self, RichText, Ui};
use ffr_core::recover::{Layout, RecoverOptions};
use ffr_core::util::human_size;
use std::time::Duration;

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let Some(session) = app.session.as_ref() else {
        ui.label(widgets::muted(&p, "Scan a disk and select files first."));
        return;
    };
    let count = app.results.sel_count;
    let bytes = app.results.sel_bytes;
    if !app.recover.running() {
        let devs: Vec<_> = app.devices.list.clone();
        app.recover
            .ensure_check(&session.source.device, &devs, count, bytes);
    }
    let can_start = count > 0
        && !app.recover.running()
        && app.recover.check.as_ref().is_some_and(|c| c.can_start())
        && !app.scan.as_ref().is_some_and(|s| s.running());
    let reason = if count == 0 {
        Some("Select files on the Results screen first.".to_string())
    } else if app.recover.dest.is_none() {
        Some("Choose a destination folder on a different disk.".to_string())
    } else if app.scan.as_ref().is_some_and(|s| s.running()) {
        Some("Wait for the scan to finish or stop it first.".to_string())
    } else {
        app.recover
            .check
            .as_ref()
            .and_then(|c| c.first_blocker())
            .map(|b| b.text.clone())
    };
    let mut start = false;
    widgets::top_bar(
        ui,
        &p,
        "Recover files",
        &format!("{count} files · {} selected", human_size(bytes)),
        |ui| {
            start =
                widgets::primary_button(ui, &p, Some(Icon::Recover), "Start recovery", can_start)
                    .clicked();
        },
    );

    if let Some(s) = app.recover.summary.clone() {
        summary(app, ui, &s);
        return;
    }
    if app.recover.running() {
        progress(app, ui);
        return;
    }

    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, &p, "Destination");
            ui.label(widgets::small_muted(&p, "Pick a folder on a different disk. Recovering to the same disk would overwrite the files you want back."));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = app
                    .recover
                    .dest
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "No folder chosen".into());
                ui.add(egui::Label::new(widgets::mono(&p, label)).truncate());
            });
            if widgets::secondary_button(ui, &p, Some(Icon::Folder), "Choose folder…", true).clicked() {
                if let Some(d) = rfd::FileDialog::new()
                    .set_title("Recover files to…")
                    .pick_folder()
                {
                    app.recover.dest = Some(d);
                    app.recover.check_key = None;
                }
            }
            if let Some(check) = &app.recover.check {
                if let Some(free) = check.items.iter().find(|i| i.text.starts_with("Enough") || i.text.starts_with("Not enough")) {
                    ui.add_space(4.0);
                    ui.label(widgets::small_muted(&p, free.text.clone()));
                }
            }
            ui.add_space(theme::SPACE);
            widgets::section_title(ui, &p, "Folder layout");
            for l in Layout::ALL {
                let desc = match l {
                    Layout::ByType => "FFR_<date>/<Category>/<type>/",
                    Layout::OriginalPaths => "Original folders; carved files in _Carved/",
                    Layout::Flat => "Everything in one folder",
                };
                ui.horizontal(|ui| {
                    if ui.radio(app.settings.layout == l, l.label()).clicked() {
                        app.settings.layout = l;
                        app.settings.save_async();
                    }
                    ui.label(widgets::small_muted(&p, desc));
                });
            }
        });
        let ui = &mut cols[1];
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, &p, "Checks");
            match &app.recover.check {
                None if app.recover.dest.is_some() => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(widgets::muted(&p, "Checking the destination…"));
                    });
                }
                None => {
                    ui.label(widgets::muted(&p, "Choose a destination to run the checks."));
                }
                Some(c) => {
                    for item in &c.items {
                        ui.horizontal(|ui| {
                            let (icon, color) = if item.ok {
                                (Icon::Check, p.success)
                            } else if item.blocking {
                                (Icon::X, p.danger)
                            } else {
                                (Icon::Alert, p.warning)
                            };
                            widgets::icons::icon(ui, icon, 16.0, color);
                            ui.add(egui::Label::new(RichText::new(&item.text).color(p.text)).wrap());
                        });
                    }
                }
            }
            ui.add_space(theme::SPACE);
            ui.label(widgets::small_muted(&p, "The source disk is only ever read. A report (CSV and JSON with BLAKE3 hashes) is written next to the files."));
        });
    });
    if let Some(r) = reason {
        ui.add_space(theme::SPACE);
        ui.label(RichText::new(r).color(p.warning));
    }

    if start {
        let Some(session) = app.session.as_ref() else {
            return;
        };
        let Some(dest) = app.recover.dest.clone() else {
            return;
        };
        let opts = RecoverOptions {
            dest,
            layout: app.settings.layout,
            partial_policy: app.settings.partial_policy,
            write_timestamps: app.settings.write_timestamps,
            retries: app.settings.retries,
            chown: crate::elevate::original_ids(),
            dest_rotational: app.recover.dest_rotational,
        };
        let sel = app.results.selection();
        let (h, rx) = ffr_core::start_recovery(session, sel, opts);
        app.recover.files_total = count as u64;
        app.recover.bytes_total = bytes;
        app.recover.files_done = 0;
        app.recover.bytes_done = 0;
        app.recover.job = Some((h, rx));
    }
}

fn progress(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let r = &app.recover;
    widgets::card(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, &p, "Recovering…");
        let f = if r.bytes_total > 0 {
            r.bytes_done as f32 / r.bytes_total as f32
        } else {
            0.0
        };
        ui.add(egui::ProgressBar::new(f).show_percentage().animate(true));
        ui.add_space(6.0);
        egui::Grid::new("recprog")
            .num_columns(2)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                ui.label(widgets::muted(&p, "Files"));
                ui.label(widgets::mono(
                    &p,
                    format!("{} / {}", r.files_done, r.files_total),
                ));
                ui.end_row();
                ui.label(widgets::muted(&p, "Data"));
                ui.label(widgets::mono(
                    &p,
                    format!(
                        "{} / {}",
                        human_size(r.bytes_done),
                        human_size(r.bytes_total)
                    ),
                ));
                ui.end_row();
                ui.label(widgets::muted(&p, "Speed"));
                ui.label(widgets::mono(&p, format!("{}/s", human_size(r.bps as u64))));
                ui.end_row();
                ui.label(widgets::muted(&p, "Current"));
                ui.add(egui::Label::new(widgets::mono(&p, r.current.clone())).truncate());
                ui.end_row();
            });
        ui.add_space(8.0);
        if widgets::danger_button(ui, &p, Some(Icon::Stop), "Stop", true).clicked() {
            if let Some((h, _)) = &app.recover.job {
                h.cancel();
            }
        }
    });
}

fn summary(app: &mut FfrApp, ui: &mut Ui, s: &ffr_core::recover::RecoverSummary) {
    let p = app.p;
    widgets::raised_card(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::icons::icon(ui, Icon::Check, 28.0, p.success);
            ui.label(
                RichText::new(if s.cancelled {
                    "Recovery stopped"
                } else {
                    "Recovery finished"
                })
                .font(theme::semibold(20.0))
                .color(p.text),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (label, n, color) in [
                ("Recovered", s.recovered, p.success),
                ("Partial", s.partial, p.warning),
                ("Failed", s.failed, p.danger),
            ] {
                egui::Frame::new()
                    .fill(Palette::soft(color, 30))
                    .corner_radius(theme::CARD_RADIUS)
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(n.to_string())
                                    .font(theme::semibold(22.0))
                                    .color(color),
                            );
                            ui.label(widgets::small_muted(&p, label));
                        });
                    });
            }
        });
        ui.add_space(8.0);
        ui.label(widgets::muted(
            &p,
            format!(
                "{} written in {} to {}",
                human_size(s.bytes),
                fmt_duration(Duration::from_secs_f64(s.elapsed_secs)),
                s.folder.display()
            ),
        ));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, &p, Some(Icon::Folder), "Open folder", true).clicked() {
                if let Err(e) = open_path(&s.folder) {
                    app.toasts.error(e);
                }
            }
            if widgets::secondary_button(ui, &p, Some(Icon::FileText), "Open report", true)
                .clicked()
            {
                if let Err(e) = open_path(&s.report_csv) {
                    app.toasts.error(e);
                }
            }
            if widgets::secondary_button(ui, &p, Some(Icon::List), "Back to results", true)
                .clicked()
            {
                app.recover.summary = None;
                app.screen = Screen::Results;
            }
        });
    });
}
