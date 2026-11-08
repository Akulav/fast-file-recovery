//! Results screen: filter rail, virtualized table, preview panel and selection footer.

use super::fmt_time;
use crate::app::{FfrApp, ResultsState, Screen, SortCol};
use crate::theme::{self, Palette};
use crate::widgets::{self, hex_view::hex_view, icons::Icon};
use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use egui_extras::{Column, TableBuilder};
use ffr_core::carve::Confidence;
use ffr_core::kinds::Category;
use ffr_core::scan::{flags, Hit, Session};
use ffr_core::util::human_size;
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn ext_of<'a>(s: &'a Session, h: &Hit) -> Cow<'a, str> {
    if h.extra == u32::MAX {
        return Cow::Borrowed(h.kind().ext);
    }
    match s.extra(h).and_then(|e| e.name.as_deref()) {
        Some(n) => match n.rsplit_once('.') {
            Some((_, e)) if !e.is_empty() && e.len() <= 8 => Cow::Owned(e.to_ascii_lowercase()),
            _ => Cow::Borrowed(h.kind().ext),
        },
        None => Cow::Borrowed(h.kind().ext),
    }
}

fn conf_color(p: &Palette, c: Confidence) -> egui::Color32 {
    match c {
        Confidence::High => p.success,
        Confidence::Medium => p.warning,
        Confidence::Low => p.danger,
    }
}

/// Rebuild the filtered + sorted index and the per-extension counts.
pub fn rebuild(r: &mut ResultsState, s: &Session) {
    r.last_build = Instant::now();
    r.dirty = false;
    if r.selected.len() < s.hits.len() {
        r.selected.resize(s.hits.len(), false);
    }
    let q = r.search.trim().to_lowercase();
    let q_off = q
        .strip_prefix("0x")
        .and_then(|h| u64::from_str_radix(h, 16).ok())
        .or_else(|| q.parse::<u64>().ok().filter(|_| q.len() >= 4));
    let mut ext_map: HashMap<String, (Category, usize, u64)> = HashMap::new();
    let mut cat_counts = [(0usize, 0u64); 6];
    let mut filtered = Vec::new();
    for (i, h) in s.hits.iter().enumerate() {
        let cat = h.kind().category;
        let ext = ext_of(s, h);
        cat_counts[cat.index()].0 += 1;
        cat_counts[cat.index()].1 += h.len;
        let e = ext_map.entry(ext.to_string()).or_insert((cat, 0, 0));
        e.1 += 1;
        e.2 += h.len;
        if !r.cat_on[cat.index()] || r.ext_off.contains(ext.as_ref()) {
            continue;
        }
        if h.len < r.min_size || (r.max_size > 0 && h.len > r.max_size) {
            continue;
        }
        if h.confidence < r.min_conf {
            continue;
        }
        let extra = s.extra(h);
        if r.named_only && extra.and_then(|e| e.name.as_ref()).is_none() {
            continue;
        }
        if !q.is_empty() {
            let hit_off = q_off.is_some_and(|o| o >= h.offset && o < h.end().max(h.offset + 1));
            let name_hit = || s.display_name(h).to_lowercase().contains(&q);
            let path_hit = || {
                extra
                    .and_then(|e| e.path.as_ref())
                    .is_some_and(|p| p.to_lowercase().contains(&q))
            };
            if !(hit_off || ext.as_ref() == q || name_hit() || path_hit()) {
                continue;
            }
        }
        filtered.push(i as u32);
    }
    let asc = r.asc;
    match r.sort {
        SortCol::Offset => filtered.sort_by_key(|&i| s.hits[i as usize].offset),
        SortCol::Size => filtered.sort_by_key(|&i| s.hits[i as usize].len),
        SortCol::Confidence => {
            filtered.sort_by_key(|&i| (s.hits[i as usize].confidence, s.hits[i as usize].offset))
        }
        SortCol::Ext => {
            let keys: HashMap<u32, String> = filtered
                .iter()
                .map(|&i| (i, ext_of(s, &s.hits[i as usize]).into_owned()))
                .collect();
            filtered.sort_by(|a, b| keys[a].cmp(&keys[b]));
        }
        SortCol::Name => {
            let keys: HashMap<u32, String> = filtered
                .iter()
                .map(|&i| (i, s.display_name(&s.hits[i as usize]).to_lowercase()))
                .collect();
            filtered.sort_by(|a, b| keys[a].cmp(&keys[b]));
        }
        SortCol::Path => {
            let keys: HashMap<u32, String> = filtered
                .iter()
                .map(|&i| {
                    let h = &s.hits[i as usize];
                    (
                        i,
                        s.extra(h)
                            .and_then(|e| e.path.clone())
                            .unwrap_or_default()
                            .to_lowercase(),
                    )
                })
                .collect();
            filtered.sort_by(|a, b| keys[a].cmp(&keys[b]));
        }
    }
    if !asc {
        filtered.reverse();
    }
    r.filtered = filtered;
    let mut exts: Vec<_> = ext_map
        .into_iter()
        .map(|(e, (c, n, b))| (e, c, n, b))
        .collect();
    exts.sort_by_key(|a| (a.1, std::cmp::Reverse(a.2)));
    r.ext_counts = exts;
    r.cat_counts = cat_counts;
}

pub fn ui(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let Some(session) = app.session.as_ref() else {
        ui.label(widgets::muted(&p, "No results yet."));
        return;
    };
    let scanning = app.scan.as_ref().is_some_and(|s| s.running());
    let throttle = if scanning {
        Duration::from_millis(600)
    } else {
        Duration::ZERO
    };
    if app.results.dirty && app.results.last_build.elapsed() >= throttle {
        rebuild(&mut app.results, session);
    }
    if app.results.dirty {
        ui.ctx().request_repaint_after(throttle);
    }

    let total = session.hits.len();
    let context = format!(
        "{} files found{} · showing {}",
        total,
        if scanning { " so far" } else { "" },
        app.results.filtered.len()
    );
    let mut go_recover = false;
    let mut save = false;
    let sel = app.results.sel_count;
    widgets::top_bar(ui, &p, "Results", &context, |ui| {
        go_recover =
            widgets::primary_button(ui, &p, Some(Icon::ArrowRight), "Recover selected", sel > 0)
                .clicked();
        save =
            widgets::secondary_button(ui, &p, Some(Icon::Save), "Save scan…", total > 0).clicked();
    });

    keyboard(app, ui);
    let Some(session) = app.session.as_ref() else {
        return;
    };

    egui::SidePanel::left("filters")
        .exact_width(232.0)
        .resizable(false)
        .frame(Frame::new().inner_margin(Margin {
            left: 0,
            right: 12,
            top: 0,
            bottom: 0,
        }))
        .show_inside(ui, |ui| filter_rail(&mut app.results, &p, ui));

    egui::TopBottomPanel::bottom("selbar")
        .frame(Frame::new().inner_margin(Margin {
            left: 0,
            right: 0,
            top: 8,
            bottom: 0,
        }))
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let r = &mut app.results;
                ui.label(
                    RichText::new(format!(
                        "{} selected · {}",
                        r.sel_count,
                        human_size(r.sel_bytes)
                    ))
                    .font(theme::semibold(13.0))
                    .color(p.text),
                );
                if ui
                    .link("Select all shown")
                    .on_hover_text("Ctrl+A")
                    .clicked()
                {
                    let ids = r.filtered.clone();
                    for id in ids {
                        r.set_selected(session, id, true);
                    }
                }
                if r.sel_count > 0 && ui.link("Clear").clicked() {
                    r.clear_selection();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary_button(
                        ui,
                        &p,
                        Some(Icon::ArrowRight),
                        "Recover selected",
                        r.sel_count > 0,
                    )
                    .clicked()
                    {
                        go_recover = true;
                    }
                });
            });
        });

    egui::SidePanel::right("preview")
        .default_width(310.0)
        .width_range(260.0..=480.0)
        .resizable(true)
        .frame(Frame::new().inner_margin(Margin {
            left: 12,
            right: 0,
            top: 0,
            bottom: 0,
        }))
        .show_inside(ui, |ui| {
            if let (Some(id), Some(s)) = (app.results.focus, app.session.as_ref()) {
                app.preview.request(id, s);
            }
            preview_panel(app, ui);
        });

    egui::CentralPanel::default()
        .frame(Frame::new())
        .show_inside(ui, |ui| table(app, ui));

    if save {
        app.save_scan_as();
    }
    if go_recover {
        app.screen = Screen::Recover;
    }
}

fn keyboard(app: &mut FfrApp, ui: &mut Ui) {
    if app.dialog.is_some() || ui.ctx().wants_keyboard_input() {
        return;
    }
    let Some(session) = app.session.as_ref() else {
        return;
    };
    let r = &mut app.results;
    let (all, space, down, up) = ui.input(|i| {
        (
            i.modifiers.command && i.key_pressed(egui::Key::A),
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
        )
    });
    if all {
        let ids = r.filtered.clone();
        for id in ids {
            r.set_selected(session, id, true);
        }
    }
    if space {
        if let Some(id) = r.focus {
            let on = !r.selected.get(id as usize).copied().unwrap_or(false);
            r.set_selected(session, id, on);
        }
    }
    if down || up {
        let pos = r
            .focus
            .and_then(|f| r.filtered.iter().position(|&x| x == f));
        let next = match (pos, down) {
            (Some(i), true) => (i + 1).min(r.filtered.len().saturating_sub(1)),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        if let Some(&id) = r.filtered.get(next) {
            r.focus = Some(id);
            r.anchor = Some(next);
            r.scroll_to = Some(next);
        }
    }
}

fn filter_rail(r: &mut ResultsState, p: &Palette, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .id_salt("filters")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut r.search)
                    .hint_text("Search name, type or 0xOffset")
                    .desired_width(f32::INFINITY),
            );
            if r.focus_search {
                resp.request_focus();
                r.focus_search = false;
            }
            if resp.changed() {
                r.dirty = true;
            }
            ui.add_space(8.0);
            ui.label(widgets::small_muted(p, "CATEGORIES"));
            for c in Category::ALL {
                let (n, bytes) = r.cat_counts[c.index()];
                let on = r.cat_on[c.index()];
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                let h = ui
                    .ctx()
                    .animate_bool_with_time(resp.id, resp.hovered(), theme::ANIM);
                ui.painter().rect_filled(
                    rect,
                    6,
                    Palette::mix(Palette::transparent(), p.raised, h),
                );
                let color = p.category(c);
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 16.0, rect.center().y),
                    Vec2::splat(16.0),
                );
                widgets::icons::paint(
                    ui,
                    icon_rect,
                    Icon::for_category(c),
                    if on { color } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 34.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    c.name(),
                    theme::semibold(13.0),
                    if on { p.text } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{n}"),
                    theme::mono(11.5),
                    p.muted,
                );
                let resp = resp.on_hover_text(format!(
                    "{n} files · {}\nClick to show/hide, double-click to show only this",
                    human_size(bytes)
                ));
                if resp.double_clicked() {
                    r.cat_on = [false; 6];
                    r.cat_on[c.index()] = true;
                    r.dirty = true;
                } else if resp.clicked() {
                    r.cat_on[c.index()] = !on;
                    r.dirty = true;
                }
            }
            if r.cat_on.iter().any(|o| !o) && ui.link("Show all categories").clicked() {
                r.cat_on = [true; 6];
                r.dirty = true;
            }
            ui.add_space(8.0);
            ui.label(widgets::small_muted(p, "FILE TYPES"));
            let exts = r.ext_counts.clone();
            for (ext, cat, n, _) in exts.iter().filter(|e| r.cat_on[e.1.index()]).take(200) {
                let mut on = !r.ext_off.contains(ext);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            r.ext_off.remove(ext);
                        } else {
                            r.ext_off.insert(ext.clone());
                        }
                        r.dirty = true;
                    }
                    ui.label(
                        RichText::new(format!(".{ext}"))
                            .font(theme::mono(12.0))
                            .color(p.category(*cat)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(widgets::small_muted(p, n.to_string()));
                    });
                });
            }
            ui.add_space(8.0);
            ui.label(widgets::small_muted(p, "SIZE"));
            let mut min_kb = (r.min_size / 1024) as f64;
            let mut max_mb = if r.max_size == 0 {
                0.0
            } else {
                (r.max_size / (1024 * 1024)) as f64
            };
            ui.horizontal(|ui| {
                ui.label(widgets::small_muted(p, "Min"));
                if ui
                    .add(
                        egui::Slider::new(&mut min_kb, 0.0..=1_048_576.0)
                            .logarithmic(true)
                            .suffix(" KB")
                            .integer(),
                    )
                    .changed()
                {
                    r.min_size = min_kb as u64 * 1024;
                    r.dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(widgets::small_muted(p, "Max"));
                if ui
                    .add(
                        egui::Slider::new(&mut max_mb, 0.0..=1_048_576.0)
                            .logarithmic(true)
                            .suffix(" MB")
                            .integer()
                            .custom_formatter(|v, _| {
                                if v <= 0.0 {
                                    "any".into()
                                } else {
                                    format!("{v:.0}")
                                }
                            }),
                    )
                    .changed()
                {
                    r.max_size = max_mb as u64 * 1024 * 1024;
                    r.dirty = true;
                }
            });
            ui.add_space(8.0);
            ui.label(widgets::small_muted(p, "CONFIDENCE"));
            let before = r.min_conf;
            egui::ComboBox::from_id_salt("conf")
                .width(200.0)
                .selected_text(match r.min_conf {
                    Confidence::Low => "Any",
                    Confidence::Medium => "Medium or high",
                    Confidence::High => "High only",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut r.min_conf, Confidence::Low, "Any");
                    ui.selectable_value(&mut r.min_conf, Confidence::Medium, "Medium or high");
                    ui.selectable_value(&mut r.min_conf, Confidence::High, "High only");
                });
            if before != r.min_conf {
                r.dirty = true;
            }
            ui.add_space(6.0);
            if ui
                .checkbox(&mut r.named_only, "Has original name")
                .changed()
            {
                r.dirty = true;
            }
        });
}

fn table(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let Some(session) = app.session.as_ref() else {
        return;
    };
    let r = &mut app.results;
    if r.filtered.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            widgets::icons::icon(ui, Icon::Search, 36.0, p.muted);
            ui.label(widgets::muted(
                &p,
                if session.hits.is_empty() {
                    "Nothing found yet."
                } else {
                    "No files match these filters."
                },
            ));
        });
        return;
    }
    let mut sort_click: Option<SortCol> = None;
    let mut row_click: Option<(usize, egui::Modifiers)> = None;
    let mut toggles: Vec<u32> = Vec::new();
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::exact(24.0))
        .column(Column::exact(22.0))
        .column(Column::remainder().at_least(150.0).clip(true))
        .column(Column::initial(56.0).at_least(40.0))
        .column(Column::initial(84.0).at_least(60.0))
        .column(Column::initial(84.0).at_least(60.0))
        .column(Column::initial(110.0).at_least(70.0))
        .column(Column::initial(210.0).at_least(80.0).clip(true))
        .min_scrolled_height(0.0);
    if let Some(row) = r.scroll_to.take() {
        table = table.scroll_to_row(row, Some(Align::Center));
    }
    let arrow = |c: SortCol| {
        if r.sort == c {
            if r.asc {
                " ▲"
            } else {
                " ▼"
            }
        } else {
            ""
        }
    };
    let headers = [
        (None, ""),
        (None, ""),
        (Some(SortCol::Name), "Name"),
        (Some(SortCol::Ext), "Type"),
        (Some(SortCol::Size), "Size"),
        (Some(SortCol::Confidence), "Confidence"),
        (Some(SortCol::Offset), "Offset"),
        (Some(SortCol::Path), "Original path · modified"),
    ];
    let header_labels: Vec<(Option<SortCol>, String)> = headers
        .iter()
        .map(|(c, l)| (*c, format!("{l}{}", c.map(arrow).unwrap_or(""))))
        .collect();
    table
        .header(28.0, |mut h| {
            for (col, label) in &header_labels {
                h.col(|ui| {
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(label)
                                .font(theme::semibold(12.5))
                                .color(p.muted),
                        )
                        .sense(Sense::click()),
                    );
                    if let Some(c) = col {
                        if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            sort_click = Some(*c);
                        }
                    }
                });
            }
        })
        .body(|body| {
            body.rows(26.0, r.filtered.len(), |mut row| {
                let idx = row.index();
                let id = r.filtered[idx];
                let h = &session.hits[id as usize];
                let extra = session.extra(h);
                let selected = r.selected.get(id as usize).copied().unwrap_or(false);
                row.set_selected(r.focus == Some(id));
                row.col(|ui| {
                    let mut v = selected;
                    if ui.checkbox(&mut v, "").changed() {
                        toggles.push(id);
                    }
                });
                row.col(|ui| {
                    let c = h.kind().category;
                    widgets::icons::icon(ui, Icon::for_category(c), 15.0, p.category(c));
                });
                row.col(|ui| {
                    let name = session.display_name(h);
                    let mut t = RichText::new(name).color(p.text);
                    if extra.and_then(|e| e.name.as_ref()).is_none() {
                        t = t.color(Palette::mix(p.text, p.muted, 0.35));
                    }
                    ui.add(egui::Label::new(t).truncate());
                    if h.has(flags::TRUNCATED) || h.has(flags::BAD_SECTORS) {
                        widgets::icons::icon(ui, Icon::Alert, 13.0, p.warning).on_hover_text(
                            if h.has(flags::BAD_SECTORS) {
                                "Overlaps unreadable sectors"
                            } else {
                                "Cut short; may be incomplete"
                            },
                        );
                    }
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(ext_of(session, h))
                            .font(theme::mono(12.0))
                            .color(p.muted),
                    );
                });
                row.col(|ui| {
                    ui.label(widgets::mono(&p, human_size(h.len)));
                });
                row.col(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 4.0, conf_color(&p, h.confidence));
                    ui.label(widgets::small_muted(&p, h.confidence.label()));
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(format!("0x{:x}", h.offset))
                            .font(theme::mono(11.5))
                            .color(p.muted),
                    );
                });
                row.col(|ui| {
                    let mut s = String::new();
                    if let Some(e) = extra {
                        if let Some(path) = &e.path {
                            s.push('/');
                            s.push_str(path);
                        }
                        if let Some(m) = e.modified {
                            if !s.is_empty() {
                                s.push_str(" · ");
                            }
                            s.push_str(&fmt_time(m));
                        }
                    }
                    ui.add(egui::Label::new(widgets::small_muted(&p, s)).truncate());
                });
                if row.response().clicked() {
                    row_click = Some((idx, row.response().ctx.input(|i| i.modifiers)));
                }
            });
        });
    for id in toggles {
        let on = !r.selected.get(id as usize).copied().unwrap_or(false);
        r.set_selected(session, id, on);
    }
    if let Some((idx, m)) = row_click {
        let id = r.filtered[idx];
        if m.shift {
            let a = r.anchor.unwrap_or(idx);
            let (lo, hi) = (a.min(idx), a.max(idx));
            let ids: Vec<u32> = r.filtered[lo..=hi].to_vec();
            for i in ids {
                r.set_selected(session, i, true);
            }
        } else if m.command {
            let on = !r.selected.get(id as usize).copied().unwrap_or(false);
            r.set_selected(session, id, on);
            r.anchor = Some(idx);
        } else {
            r.anchor = Some(idx);
        }
        r.focus = Some(id);
    }
    if let Some(c) = sort_click {
        if r.sort == c {
            r.asc = !r.asc;
        } else {
            r.sort = c;
            r.asc = !matches!(c, SortCol::Size | SortCol::Confidence);
        }
        r.dirty = true;
    }
}

fn preview_panel(app: &mut FfrApp, ui: &mut Ui) {
    let p = app.p;
    let Some(session) = app.session.as_ref() else {
        return;
    };
    let Some(id) = app.results.focus else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            widgets::icons::icon(ui, Icon::Image, 36.0, p.muted);
            ui.label(widgets::muted(&p, "Select a file to preview it."));
        });
        return;
    };
    let Some(h) = session.hits.get(id as usize) else {
        return;
    };
    let extra = session.extra(h);
    egui::ScrollArea::vertical()
        .id_salt("preview")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(session.display_name(h))
                        .font(theme::semibold(15.0))
                        .color(p.text),
                )
                .wrap(),
            );
            ui.label(widgets::small_muted(&p, h.kind().description));
            ui.add_space(8.0);
            let frame = Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(theme::CARD_RADIUS)
                .inner_margin(Margin::same(8));
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                match app.preview.get(id) {
                    None => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(widgets::small_muted(&p, "Loading preview…"));
                        });
                        ui.add_space(20.0);
                    }
                    Some(pv) => {
                        if let Some(tex) = &pv.texture {
                            let size = tex.size_vec2();
                            let w = ui.available_width();
                            let scale = (w / size.x).min(320.0 / size.y).min(2.0);
                            ui.vertical_centered(|ui| {
                                ui.add(egui::Image::new((tex.id(), size * scale)).corner_radius(6));
                            });
                        } else if let Some(text) = &pv.text {
                            egui::ScrollArea::vertical()
                                .id_salt("textpv")
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(text)
                                                .font(theme::mono(11.5))
                                                .color(p.text),
                                        )
                                        .wrap(),
                                    );
                                });
                        }
                        if let Some(e) = &pv.error {
                            widgets::banner(ui, &p, p.warning, Icon::Alert, e);
                        }
                        if pv.texture.is_none() && pv.text.is_none() {
                            ui.set_height(260.0);
                            hex_view(ui, &p, &pv.hex, h.offset);
                        }
                    }
                }
            });
            ui.add_space(8.0);
            egui::Grid::new("meta")
                .num_columns(2)
                .spacing([12.0, 5.0])
                .show(ui, |ui| {
                    let mut row = |k: &str, v: String| {
                        ui.label(widgets::small_muted(&p, k));
                        ui.add(egui::Label::new(widgets::mono(&p, v)).wrap());
                        ui.end_row();
                    };
                    row("Size", format!("{} ({} bytes)", human_size(h.len), h.len));
                    row("Offset", format!("0x{:x}", h.offset));
                    row("Confidence", h.confidence.label().into());
                    row(
                        "Found by",
                        if h.has(flags::QUICK) {
                            "Quick scan (file system)".into()
                        } else {
                            "Deep scan (content)".into()
                        },
                    );
                    if let Some(e) = extra {
                        if let Some(m) = &e.meta {
                            if m.width > 0 {
                                row("Dimensions", format!("{} × {}", m.width, m.height));
                            }
                            if m.duration_ms > 0 {
                                row(
                                    "Duration",
                                    super::fmt_duration(Duration::from_millis(
                                        m.duration_ms as u64,
                                    )),
                                );
                            }
                            if let Some(t) = &m.title {
                                row("Title", t.clone());
                            }
                        }
                        if let Some(path) = &e.path {
                            row("Original path", format!("/{path}"));
                        }
                        if let Some(t) = e.modified {
                            row("Modified", fmt_time(t));
                        }
                        if let Some(t) = e.created {
                            row("Created", fmt_time(t));
                        }
                        if e.extents.len() > 1 {
                            row("Fragments", e.extents.len().to_string());
                        }
                    }
                    if h.has(flags::TRUNCATED) {
                        row("Note", "Cut short; may be incomplete".into());
                    }
                    if h.has(flags::BAD_SECTORS) {
                        row("Note", "Overlaps unreadable sectors".into());
                    }
                });
            if let Some(pv) = app.preview.get(id) {
                if pv.texture.is_some() || pv.text.is_some() {
                    ui.add_space(8.0);
                    egui::CollapsingHeader::new("First 4 KiB (hex)")
                        .id_salt("hexc")
                        .show(ui, |ui| {
                            ui.set_height(220.0);
                            hex_view(ui, &p, &pv.hex, h.offset);
                        });
                }
            }
        });
}
