//! Category chip: icon, name, live count and size.

use super::icons::{self, Icon};
use crate::theme::{self, Palette};
use eframe::egui::{self, CornerRadius, Response, Sense, Stroke, Ui, Vec2};

pub fn category_chip(
    ui: &mut Ui,
    p: &Palette,
    icon: Icon,
    color: egui::Color32,
    label: &str,
    detail: &str,
    selected: bool,
) -> Response {
    let title = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::semibold(13.0), p.text);
    let sub = ui
        .painter()
        .layout_no_wrap(detail.to_string(), theme::mono(11.5), p.muted);
    let w = 12.0 + 28.0 + 8.0 + title.size().x.max(sub.size().x) + 14.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.max(120.0), 48.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let h = ui
            .ctx()
            .animate_bool_with_time(resp.id, resp.hovered(), theme::ANIM);
        let s = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("s"), selected, theme::ANIM);
        let fill = Palette::mix(
            Palette::mix(p.surface, p.raised, h),
            Palette::soft(color, 40),
            s * 0.6,
        );
        let stroke = Palette::mix(p.border, color, s);
        ui.painter().rect(
            rect,
            CornerRadius::same(theme::CARD_RADIUS),
            fill,
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Inside,
        );
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 12.0, rect.center().y - 14.0),
            Vec2::splat(28.0),
        );
        ui.painter()
            .rect_filled(icon_rect, CornerRadius::same(8), Palette::soft(color, 45));
        icons::paint(ui, icon_rect.shrink(6.0), icon, color);
        let x = icon_rect.right() + 8.0;
        ui.painter()
            .galley(egui::pos2(x, rect.top() + 7.0), title, p.text);
        ui.painter()
            .galley(egui::pos2(x, rect.top() + 26.0), sub, p.muted);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
