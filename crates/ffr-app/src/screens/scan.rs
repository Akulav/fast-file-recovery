//! Scan screen: progress ring, throughput sparkline, sector map, category counters.

use super::fmt_duration;
use crate::app::{FfrApp, Screen};
use crate::theme::{self, Palette};
use crate::widgets::{
    self, chip::category_chip, icons::Icon, progress_ring::progress_ring, sector_map::sector_map,
    sparkline::sparkline,
};
use eframe::egui::{self, RichText, Ui};
use ffr_core::kinds::Category;
use ffr_core::scan::Phase;
use ffr_core::util::human_size;

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let Some(scan) = &app.scan else {
        ui.label(widgets::muted(
            &p,
            "No scan yet. Pick a source and start one.",
        ));
        return;
    };
    let running = scan.running();
    let paused = scan.handle.is_paused();
    let mut context = scan.title.clone();
    match &scan.lock_desc {
        Some(d) => context.push_str(&format!(" · locked 🔒 ({d})")),
        None if scan.opts.lock => {}
        None => context.push_str(" · not locked"),
    }
    let (mut pause, mut stop, mut view, mut again) = (false, false, false, false);
    let has_hits = app.session.as_ref().is_some_and(|s| !s.hits.is_empty());
    widgets::top_bar(ui, &p, "Scanning", &context, |ui| {
        view = widgets::primary_button(
            ui,
            &p,
            Some(Icon::List),
            "View results",
            has_hits || !running,
        )
        .clicked();
        if running {
            stop = widgets::danger_button(ui, &p, Some(Icon::Stop), "Stop", true).clicked();
            pause = widgets::secondary_button(
                ui,
                &p,
                Some(if paused { Icon::Play } else { Icon::Pause }),
                if paused { "Resume" } else { "Pause" },
                true,
            )
            .clicked();
        } else {
            again =
                widgets::secondary_button(ui, &p, Some(Icon::Refresh), "New scan", true).clicked();
        }
    });

    let status = if let Some(e) = &scan.failed {
        format!("Failed: {e}")
    } else if let Some(d) = &scan.done {
        if d.cancelled {
            "Stopped (saved; you can resume it)".to_string()
        } else {
            format!("Finished in {}", fmt_duration(d.elapsed))
        }
    } else if paused {
        "Paused".to_string()
    } else {
        scan.phase.label().to_string()
    };

    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(300.0);
            ui.vertical_centered(|ui| {
                let quick = scan.phase == Phase::QuickScan || scan.phase == Phase::Preparing;
                let frac = if scan.done.is_some() {
                    Some(1.0)
                } else if scan.bytes_total == 0 && running {
                    None
                } else {
                    Some(scan.smooth)
                };
                let big = match frac {
                    Some(f) => format!("{:.0}%", f * 100.0),
                    None => "…".into(),
                };
                let small = if quick && running {
                    "reading records".to_string()
                } else {
                    format!(
                        "{} / {}",
                        human_size(scan.bytes_done),
                        human_size(scan.bytes_total)
                    )
                };
                let color = if scan.failed.is_some() {
                    p.danger
                } else if paused {
                    p.warning
                } else if scan.done.is_some() {
                    p.success
                } else {
                    p.accent
                };
                progress_ring(ui, &p, 230.0, frac, &big, &small, color);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(status)
                        .font(theme::semibold(14.0))
                        .color(p.text),
                );
            });
        });
        let ui = &mut cols[1];
        widgets::card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(300.0);
            widgets::section_title(ui, &p, "Speed");
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}/s", human_size(scan.bps as u64)))
                        .font(theme::mono(22.0))
                        .color(p.text),
                );
            });
            sparkline(
                ui,
                &p,
                &scan.throughput.iter().copied().collect::<Vec<_>>(),
                90.0,
                p.accent,
            );
            ui.add_space(theme::SPACE);
            egui::Grid::new("scanstats")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    ui.label(widgets::muted(&p, "Elapsed"));
                    ui.label(widgets::mono(&p, fmt_duration(scan.elapsed())));
                    ui.end_row();
                    ui.label(widgets::muted(&p, "Time left"));
                    ui.label(widgets::mono(
                        &p,
                        match (scan.eta, running && !paused) {
                            (Some(e), true) => fmt_duration(e),
                            _ => "—".into(),
                        },
                    ));
                    ui.end_row();
                    ui.label(widgets::muted(&p, "Phase"));
                    ui.label(widgets::mono(&p, scan.phase.label()));
                    ui.end_row();
                    ui.label(widgets::muted(&p, "Unreadable areas"));
                    ui.label(widgets::mono(&p, scan.bad_ranges.to_string()));
                    ui.end_row();
                });
        });
    });
    ui.add_space(theme::SPACE);
    widgets::card(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::section_title(ui, &p, "Sector map");
            ui.add_space(12.0);
            for (c, label) in [
                (p.map_unread, "not read"),
                (Palette::soft(p.accent, 120), "read"),
                (p.success, "files found"),
                (p.danger, "unreadable"),
            ] {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2, c);
                ui.label(widgets::small_muted(&p, label));
            }
        });
        sector_map(ui, &p, &scan.sector_map, 110.0);
    });
    ui.add_space(theme::SPACE);
    let mut jump: Option<Category> = None;
    ui.horizontal_wrapped(|ui| {
        for c in Category::ALL {
            let (n, bytes) = app.category_totals[c.index()];
            let detail = format!("{n} · {}", human_size(bytes));
            if category_chip(
                ui,
                &p,
                Icon::for_category(c),
                p.category(c),
                c.name(),
                &detail,
                false,
            )
            .clicked()
            {
                jump = Some(c);
            }
        }
    });

    if pause {
        app.toggle_pause();
    }
    if stop {
        app.stop_scan();
    }
    if view {
        app.screen = Screen::Results;
    }
    if again {
        app.screen = Screen::Source;
    }
    if let Some(c) = jump {
        app.results.cat_on = [false; 6];
        app.results.cat_on[c.index()] = true;
        app.results.dirty = true;
        app.screen = Screen::Results;
    }
}
