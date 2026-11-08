//! Settings screen: appearance, scanning, formats, recovery, updates, advanced.

use crate::app::FfrApp;
use crate::settings::{Settings, ThemePref};
use crate::theme;
use crate::widgets::{self, icons::Icon};
use eframe::egui::{self, RichText, Ui};
use ffr_core::kinds::Category;
use ffr_core::recover::{Layout, PartialPolicy};
use ffr_core::scan::ScanMode;
use ffr_core::signatures::{self, Preset};
use std::collections::BTreeSet;

pub fn preset_name(enabled: &BTreeSet<String>) -> String {
    Preset::ALL
        .iter()
        .find(|p| p.ids() == *enabled)
        .map(|p| p.label().to_string())
        .unwrap_or_else(|| "Custom".into())
}

fn row(ui: &mut Ui, p: &theme::Palette, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(260.0);
            ui.label(RichText::new(label).color(p.text));
            if !hint.is_empty() {
                ui.label(widgets::small_muted(p, hint));
            }
        });
        add(ui);
    });
    ui.add_space(6.0);
}

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    widgets::top_bar(ui, &p, "Settings", "Saved automatically", |_| {});
    let before = app.settings.clone();
    let s = &mut app.settings;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_title(ui, &p, "Appearance");
                row(ui, &p, "Theme", "", |ui| {
                    widgets::segmented(
                        ui,
                        &p,
                        &mut s.theme,
                        &[
                            (ThemePref::Dark, "Dark"),
                            (ThemePref::Light, "Light"),
                            (ThemePref::System, "Follow system"),
                        ],
                    );
                });
            });
            ui.add_space(theme::SPACE);
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_title(ui, &p, "Scanning");
                row(ui, &p, "Default mode", "", |ui| {
                    widgets::segmented(
                        ui,
                        &p,
                        &mut s.default_mode,
                        &[
                            (ScanMode::Quick, "Quick"),
                            (ScanMode::Deep, "Deep"),
                            (ScanMode::Full, "Full"),
                        ],
                    );
                });
                row(ui, &p, "Chunk size", "Bigger chunks suit fast disks", |ui| {
                    ui.add(egui::Slider::new(&mut s.chunk_mib, 4..=64).suffix(" MiB"));
                });
                row(ui, &p, "Queue depth", "Parallel readers for SSD/NVMe (0 = automatic)", |ui| {
                    ui.add(egui::Slider::new(&mut s.queue_depth, 0..=16));
                });
                row(ui, &p, "Retries on read errors", "Per sector, with a growing delay", |ui| {
                    ui.add(egui::Slider::new(&mut s.retries, 0..=10));
                });
                row(
                    ui,
                    &p,
                    "Only cluster-aligned starts",
                    "File systems start files on cluster boundaries; this cuts false hits a lot",
                    |ui| {
                        widgets::toggle(ui, &p, &mut s.cluster_aligned_only);
                    },
                );
            });
            ui.add_space(theme::SPACE);
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    widgets::section_title(ui, &p, "Formats");
                    ui.label(widgets::small_muted(&p, format!("{} on", s.enabled_formats.len())));
                });
                ui.horizontal(|ui| {
                    for preset in Preset::ALL {
                        if widgets::secondary_button(ui, &p, None, preset.label(), true).clicked() {
                            s.enabled_formats = preset.ids();
                        }
                    }
                    if widgets::secondary_button(ui, &p, None, "None", true).clicked() {
                        s.enabled_formats.clear();
                    }
                });
                ui.add_space(6.0);
                ui.columns(3, |cols| {
                    for (i, c) in Category::ALL.iter().enumerate() {
                        let ui = &mut cols[i % 3];
                        ui.horizontal(|ui| {
                            widgets::icons::icon(ui, Icon::for_category(*c), 15.0, p.category(*c));
                            ui.label(RichText::new(c.name()).font(theme::semibold(13.0)).color(p.text));
                        });
                        for sig in signatures::all().iter().filter(|g| g.category == *c) {
                            let mut on = s.enabled_formats.contains(sig.id);
                            let mut label = sig.name.to_string();
                            if !sig.default_enabled {
                                label.push_str(" ·");
                            }
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    s.enabled_formats.insert(sig.id.to_string());
                                } else {
                                    s.enabled_formats.remove(sig.id);
                                }
                            }
                        }
                        ui.add_space(8.0);
                    }
                });
                ui.label(widgets::small_muted(&p, "Formats marked · are off by default because they match a lot of random text."));
            });
            ui.add_space(theme::SPACE);
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_title(ui, &p, "Recovery");
                row(ui, &p, "Folder layout", "", |ui| {
                    egui::ComboBox::from_id_salt("layout")
                        .selected_text(s.layout.label())
                        .show_ui(ui, |ui| {
                            for l in Layout::ALL {
                                ui.selectable_value(&mut s.layout, l, l.label());
                            }
                        });
                });
                row(ui, &p, "Files with unreadable sectors", "", |ui| {
                    egui::ComboBox::from_id_salt("partial")
                        .selected_text(match s.partial_policy {
                            PartialPolicy::Suffix => "Add .partial to the name",
                            PartialPolicy::ZeroFill => "Zero-fill, note in report",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.partial_policy, PartialPolicy::Suffix, "Add .partial to the name");
                            ui.selectable_value(&mut s.partial_policy, PartialPolicy::ZeroFill, "Zero-fill, note in report");
                        });
                });
                row(ui, &p, "Restore original timestamps", "When the file system recorded them", |ui| {
                    widgets::toggle(ui, &p, &mut s.write_timestamps);
                });
            });
            ui.add_space(theme::SPACE);
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_title(ui, &p, "Updates");
                row(ui, &p, "Check automatically", "Looks for a new release on GitHub every 6 hours. Nothing installs without your click.", |ui| {
                    widgets::toggle(ui, &p, &mut s.auto_update);
                });
            });
            ui.add_space(theme::SPACE);
            let mut reset = false;
            widgets::card(&p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_title(ui, &p, "Advanced");
                ui.horizontal(|ui| {
                    if widgets::secondary_button(ui, &p, Some(Icon::Folder), "Open log folder", true).clicked() {
                        let _ = std::fs::create_dir_all(crate::paths::logs_dir());
                        if let Err(e) = crate::app::open_path(&crate::paths::logs_dir()) {
                            tracing::warn!("{e}");
                        }
                    }
                    if widgets::danger_button(ui, &p, Some(Icon::Refresh), "Reset settings", true).clicked() {
                        reset = true;
                    }
                });
                ui.label(widgets::small_muted(&p, format!("Settings: {}", crate::paths::settings_file().display())));
            });
            if reset {
                let window = s.window;
                *s = Settings {
                    window,
                    ..Settings::default()
                };
            }
        });
    if app.settings != before {
        app.preset_label = preset_name(&app.settings.enabled_formats);
        app.settings.save_async();
    }
}
