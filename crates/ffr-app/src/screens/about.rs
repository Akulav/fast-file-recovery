//! About screen: identity, version, updates and release notes.

use crate::app::FfrApp;
use crate::branding::{self, APP_NAME, LICENSE, MADE_BY, REPO_URL, TAGLINE, VERSION};
use crate::theme;
use crate::updater::{UpdateState, Updater};
use crate::widgets::{self, icons::Icon};
use eframe::egui::{self, RichText, Ui, Vec2};
use ffr_core::util::human_size;

/// Very small markdown renderer for release notes (headings, bullets, paragraphs).
fn markdown(ui: &mut Ui, p: &theme::Palette, text: &str) {
    for line in text.lines() {
        let t = line.trim_end();
        if let Some(h) = t
            .strip_prefix("### ")
            .or_else(|| t.strip_prefix("## "))
            .or_else(|| t.strip_prefix("# "))
        {
            ui.add_space(4.0);
            ui.label(RichText::new(h).font(theme::semibold(14.0)).color(p.text));
        } else if let Some(b) = t
            .trim_start()
            .strip_prefix("- ")
            .or_else(|| t.trim_start().strip_prefix("* "))
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(p.accent));
                ui.label(RichText::new(b.replace("**", "").replace('`', "")).color(p.text));
            });
        } else if t.is_empty() {
            ui.add_space(4.0);
        } else {
            ui.label(RichText::new(t.replace("**", "").replace('`', "")).color(p.text));
        }
    }
}

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    widgets::top_bar(ui, &p, "About", TAGLINE, |_| {});
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::from_bytes("bytes://ffr-icon-256.png", branding::ICON_256)
                        .fit_to_exact_size(Vec2::splat(96.0)),
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(APP_NAME).font(theme::semibold(24.0)).color(p.text));
                    ui.label(RichText::new(format!("Version {VERSION}")).font(theme::mono(13.0)).color(p.muted));
                    ui.label(RichText::new(MADE_BY).font(theme::semibold(13.0)).color(p.accent));
                    ui.hyperlink_to(REPO_URL, REPO_URL);
                    ui.label(widgets::small_muted(&p, format!("{LICENSE} · {}", branding::os_arch())));
                    if crate::elevate::is_elevated() {
                        ui.label(widgets::small_muted(&p, "Running with administrator rights"));
                    }
                });
            });
        });
        ui.add_space(theme::SPACE);
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, &p, "Updates");
            if let Some(reason) = Updater::disabled_reason() {
                ui.label(widgets::muted(&p, reason));
                return;
            }
            let job = app.job_running();
            let mut check = false;
            let mut update = false;
            ui.horizontal(|ui| {
                check = widgets::secondary_button(ui, &p, Some(Icon::Refresh), "Check for updates", !app.updater.busy()).clicked();
                if let Some(r) = app.updater.available() {
                    let label = format!("Update to v{}", r.version);
                    let resp = widgets::primary_button(ui, &p, Some(Icon::Download), &label, !job);
                    update = resp.clicked();
                    if job {
                        resp.on_hover_text("Finish or stop the current job first.");
                    }
                }
            });
            if job && app.updater.available().is_some() {
                ui.label(RichText::new("Finish or stop the current job first.").color(p.warning));
            }
            match &app.updater.state {
                UpdateState::Idle => {
                    ui.label(widgets::muted(&p, "Not checked yet."));
                }
                UpdateState::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(widgets::muted(&p, "Checking…"));
                    });
                }
                UpdateState::UpToDate => {
                    ui.label(RichText::new("You have the latest version.").color(p.success));
                }
                UpdateState::Available(r) => {
                    ui.label(RichText::new(format!("Version {} is available.", r.version)).color(p.accent));
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(p.bg)
                        .corner_radius(theme::CONTROL_RADIUS)
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            markdown(ui, &p, &r.notes);
                        });
                }
                UpdateState::Downloading { done, total } => {
                    let f = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(f).show_percentage().animate(true));
                    ui.label(widgets::mono(&p, format!("{} / {}", human_size(*done), human_size(*total))));
                }
                UpdateState::Verifying => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(widgets::muted(&p, "Verifying SHA-256…"));
                    });
                }
                UpdateState::Applying => {
                    ui.label(widgets::muted(&p, "Installing the update; the app will restart."));
                }
                UpdateState::Error(e) => {
                    widgets::banner(ui, &p, p.danger, Icon::Alert, e);
                }
            }
            if check {
                app.updater.check_now();
            }
            if update && !job {
                app.updater.start_update();
            }
        });
        ui.add_space(theme::SPACE);
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, &p, "Safety and privacy");
            for line in [
                "The source disk is opened read-only. Nothing is ever written to it.",
                "Recovered files must go to a different physical disk; the app enforces this.",
                "The disk is locked during a scan so the system can't reuse freed space.",
                "No telemetry. The network is used only to check for updates.",
            ] {
                ui.horizontal(|ui| {
                    widgets::icons::icon(ui, Icon::Check, 14.0, p.success);
                    ui.label(RichText::new(line).color(p.text));
                });
            }
            if cfg!(target_os = "macos") {
                ui.add_space(6.0);
                ui.label(widgets::small_muted(&p, "macOS: give the app Full Disk Access (System Settings → Privacy & Security) to read external disks. Internal sealed system volumes on Apple Silicon can't be read raw; external disks and images work."));
            }
        });
    });
}
