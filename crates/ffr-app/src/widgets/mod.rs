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

/// A glass card with a hairline border and a barely-there resting shadow.
pub fn card(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.glass())
        .stroke(Stroke::new(1.0_f32, p.glass_border()))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(18))
        .shadow(p.rest_shadow())
}

/// A raised card with a soft shadow.
pub fn raised_card(p: &Palette) -> Frame {
    card(p).fill(p.raised).shadow(p.soft_shadow())
}

/// Keyboard focus ring around a custom-drawn control.
pub fn focus_ring(ui: &Ui, p: &Palette, resp: &Response, radius: u8) {
    if resp.has_focus() {
        ui.painter().rect_stroke(
            resp.rect.expand(2.5),
            CornerRadius::same(radius + 2),
            Stroke::new(2.0_f32, p.focus()),
            egui::StrokeKind::Outside,
        );
    }
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
    let size = Vec2::new(galley.size().x + icon_w + 30.0, 34.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
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
        let radius = CornerRadius::same(CONTROL_RADIUS);
        if primary && enabled {
            // Soft coloured glow that grows on hover.
            painter.add(
                egui::epaint::Shadow {
                    offset: [0, 2],
                    blur: (6.0 + 8.0 * t) as u8,
                    spread: 0,
                    color: Palette::soft(base, (40.0 + 50.0 * t) as u8),
                }
                .as_shape(rect, radius),
            );
        }
        painter.rect(
            rect,
            radius,
            fill,
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Inside,
        );
        if primary && enabled {
            // Thin top highlight gives the fill a little depth.
            let top = egui::Rect::from_min_max(
                rect.min + Vec2::new(2.0, 1.0),
                egui::pos2(rect.right() - 2.0, rect.top() + 1.5),
            );
            painter.rect_filled(top, 0, Palette::soft(Palette::white(), 46));
        }
        let mut x = rect.left() + 15.0;
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
        focus_ring(ui, p, &resp, CONTROL_RADIUS);
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
    let r = Frame::new()
        .fill(Palette::soft(color, 22))
        .stroke(Stroke::new(1.0_f32, Palette::soft(color, 80)))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS + 2))
        .inner_margin(Margin {
            left: 16,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::icon(ui, icon, 16.0, color);
                ui.add_space(2.0);
                ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
            });
        });
    // Accent stripe on the leading edge.
    let rect = r.response.rect;
    let stripe = egui::Rect::from_min_max(
        rect.min + Vec2::new(0.0, 0.0),
        egui::pos2(rect.left() + 3.0, rect.bottom()),
    );
    ui.painter().rect_filled(
        stripe,
        CornerRadius {
            nw: CONTROL_RADIUS + 2,
            sw: CONTROL_RADIUS + 2,
            ne: 0,
            se: 0,
        },
        color,
    );
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
        .fill(p.inset())
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
                    let r = CornerRadius::same(CONTROL_RADIUS);
                    if t > 0.0 {
                        ui.painter().add(
                            egui::epaint::Shadow {
                                offset: [0, 1],
                                blur: 6,
                                spread: 0,
                                color: Palette::soft(p.accent, (70.0 * t) as u8),
                            }
                            .as_shape(rect, r),
                        );
                    }
                    let idle = Palette::mix(p.inset(), p.raised, h);
                    let fill = Palette::mix(idle, p.accent, t);
                    ui.painter().rect_filled(rect, r, fill);
                    let fg = Palette::mix(Palette::mix(p.muted, p.text, h), p.on_accent, t);
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
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, ""));
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, theme::ANIM);
    let h = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("h"), resp.hovered(), theme::ANIM);
    let off_track = Palette::mix(p.border, p.muted, 0.25 * h);
    let track = Palette::mix(off_track, p.accent, t);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    let knob = egui::pos2(x, rect.center().y);
    ui.painter().circle_filled(
        knob + Vec2::new(0.0, 1.0),
        7.5,
        Palette::soft(Color32::BLACK, 40),
    );
    ui.painter().circle_filled(knob, 7.0, Palette::white());
    focus_ring(ui, p, &resp, 10);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Screen top bar: title, one-line context, and an action area on the right.
pub fn top_bar(ui: &mut Ui, p: &Palette, title: &str, context: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(title)
                    .font(theme::semibold(22.0))
                    .color(p.text),
            );
            ui.label(muted(p, context));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(theme::SPACE * 1.5);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.divider()));
    ui.add_space(theme::SPACE * 2.0);
}

/// Smoothly approach a target value (for progress numbers).
pub fn lerp_towards(current: &mut f32, target: f32, dt: f32) {
    let k = 1.0 - (-dt * 10.0).exp();
    *current += (target - *current) * k;
    if (*current - target).abs() < 0.0005 {
        *current = target;
    }
}
