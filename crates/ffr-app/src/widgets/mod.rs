//! Reusable widgets, all drawn from the theme palette.

pub mod chip;
pub mod disk_card;
pub mod hex_view;
pub mod icons;
pub mod progress_ring;
pub mod sector_map;
pub mod sparkline;
pub mod toast;

use crate::theme::{self, Palette, CARD_RADIUS, CONTROL_RADIUS};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, Response, RichText, Sense, Stroke,
    Ui, Vec2,
};
use icons::Icon;

/// A surface card.
pub fn card(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(16))
}

/// A raised card with a soft shadow.
pub fn raised_card(p: &Palette) -> Frame {
    card(p).fill(p.raised).shadow(egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: p.shadow,
    })
}

fn button_inner(
    ui: &mut Ui,
    p: &Palette,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
    primary: bool,
    danger: bool,
) -> Response {
    let font = theme::semibold(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, p.text);
    let icon_w = if icon.is_some() { 16.0 + 6.0 } else { 0.0 };
    let size = Vec2::new(galley.size().x + icon_w + 28.0, 32.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let t = ui
            .ctx()
            .animate_bool_with_time(resp.id, resp.hovered() && enabled, theme::ANIM);
        let (base, hover, fg, stroke) = if primary {
            let base = if danger { p.danger } else { p.accent };
            let hover = if danger {
                Palette::mix(p.danger, Palette::white(), 0.12)
            } else {
                p.accent_hover
            };
            (base, hover, p.on_accent, Palette::transparent())
        } else {
            (
                p.raised,
                Palette::mix(p.raised, p.accent, 0.14),
                if danger { p.danger } else { p.text },
                p.border,
            )
        };
        let mut fill = Palette::mix(base, hover, t);
        let mut fg = fg;
        if !enabled {
            fill = Palette::mix(fill, p.bg, 0.55);
            fg = Palette::mix(fg, p.bg, 0.5);
        }
        if resp.is_pointer_button_down_on() && enabled {
            fill = Palette::mix(fill, p.bg, 0.15);
        }
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(CONTROL_RADIUS),
            fill,
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left() + 14.0;
        if let Some(ic) = icon {
            let r = egui::Rect::from_center_size(
                egui::pos2(x + 8.0, rect.center().y),
                Vec2::splat(16.0),
            );
            icons::paint(ui, r, ic, fg);
            x += icon_w;
        }
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_string(), theme::semibold(13.0), fg);
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, fg);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn primary_button(
    ui: &mut Ui,
    p: &Palette,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
) -> Response {
    button_inner(ui, p, icon, text, enabled, true, false)
}

pub fn secondary_button(
    ui: &mut Ui,
    p: &Palette,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
) -> Response {
    button_inner(ui, p, icon, text, enabled, false, false)
}

pub fn danger_button(
    ui: &mut Ui,
    p: &Palette,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
) -> Response {
    button_inner(ui, p, icon, text, enabled, false, true)
}

/// Section title (15 px semibold).
pub fn section_title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::semibold(15.0))
            .color(p.text),
    );
}

pub fn muted(p: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(p.muted)
}

pub fn small_muted(p: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(p.muted).size(12.0)
}

pub fn mono(p: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(theme::mono(12.5))
        .color(p.text)
}

/// A coloured banner (warning / error / info) with an icon.
pub fn banner(ui: &mut Ui, p: &Palette, color: Color32, icon: Icon, text: &str) {
    Frame::new()
        .fill(Palette::soft(color, 28))
        .stroke(Stroke::new(1.0_f32, Palette::soft(color, 110)))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::icon(ui, icon, 16.0, color);
                ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
            });
        });
}

/// A segmented control. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(p.bg)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS + 2))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let selected = *value == *v;
                    let galley = ui.painter().layout_no_wrap(
                        label.to_string(),
                        theme::semibold(13.0),
                        p.text,
                    );
                    let size = Vec2::new(galley.size().x + 28.0, 28.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    let t = ui
                        .ctx()
                        .animate_bool_with_time(resp.id, selected, theme::ANIM);
                    let h = ui.ctx().animate_bool_with_time(
                        resp.id.with("h"),
                        resp.hovered(),
                        theme::ANIM,
                    );
                    let fill = Palette::mix(Palette::mix(p.bg, p.raised, h), p.accent, t);
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
                    let fg = Palette::mix(p.muted, p.on_accent, t);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        *label,
                        theme::semibold(13.0),
                        fg,
                    );
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                        && !selected
                    {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A pill-shaped toggle switch.
pub fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let size = Vec2::new(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, theme::ANIM);
    let track = Palette::mix(p.border, p.accent, t);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 7.0, Palette::white());
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Screen top bar: title, one-line context, and an action area on the right.
pub fn top_bar(ui: &mut Ui, p: &Palette, title: &str, context: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .font(theme::semibold(22.0))
                    .color(p.text),
            );
            ui.label(muted(p, context));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(theme::SPACE);
}

/// Smoothly approach a target value (for progress numbers).
pub fn lerp_towards(current: &mut f32, target: f32, dt: f32) {
    let k = 1.0 - (-dt * 10.0).exp();
    *current += (target - *current) * k;
    if (*current - target).abs() < 0.0005 {
        *current = target;
    }
}
