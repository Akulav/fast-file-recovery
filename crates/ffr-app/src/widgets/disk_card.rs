//! Disk card: bus icon, model, size, badges and a clickable partition bar.

use super::icons::{self, Icon};
use crate::theme::{self, Palette};
use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, Ui, Vec2};
use ffr_core::device::{DeviceInfo, DeviceKind, FsType};
use ffr_core::util::human_size;

pub struct CardResponse {
    pub clicked: bool,
    /// A partition segment was clicked (scan just that partition).
    pub partition: Option<u32>,
}

fn fs_color(p: &Palette, fs: &FsType) -> egui::Color32 {
    match fs {
        FsType::Ntfs => p.images,
        FsType::Fat12 | FsType::Fat16 | FsType::Fat32 | FsType::ExFat => p.documents,
        FsType::Ext | FsType::Btrfs | FsType::Xfs => p.audio,
        FsType::Apfs | FsType::HfsPlus => p.video,
        FsType::Iso9660 => p.archives,
        _ => p.muted,
    }
}

pub fn disk_card(
    ui: &mut Ui,
    p: &Palette,
    dev: &DeviceInfo,
    selected: bool,
    selected_partition: Option<u32>,
    width: f32,
) -> CardResponse {
    let height = 132.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let mut out = CardResponse {
        clicked: false,
        partition: None,
    };
    if !ui.is_rect_visible(rect) {
        return out;
    }
    let h = ui
        .ctx()
        .animate_bool_with_time(resp.id, resp.hovered(), theme::ANIM);
    let s = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("sel"), selected, theme::ANIM);
    let fill = Palette::mix(p.surface, p.raised, h.max(s));
    let stroke = Palette::mix(Palette::mix(p.border, p.muted, h * 0.5), p.accent, s);
    if s > 0.0 {
        ui.painter().rect_filled(
            rect.expand(3.0 * s),
            CornerRadius::same(theme::CARD_RADIUS + 3),
            Palette::soft(p.accent, (40.0 * s) as u8),
        );
    }
    ui.painter().rect(
        rect,
        CornerRadius::same(theme::CARD_RADIUS),
        fill,
        Stroke::new(if selected { 1.5_f32 } else { 1.0_f32 }, stroke),
        egui::StrokeKind::Inside,
    );
    let pad = 14.0;
    let icon_rect = Rect::from_min_size(rect.min + Vec2::splat(pad), Vec2::splat(36.0));
    ui.painter().rect_filled(
        icon_rect,
        CornerRadius::same(9),
        Palette::soft(p.accent, 40),
    );
    icons::paint(
        ui,
        icon_rect.shrink(8.0),
        Icon::for_bus(dev.bus, dev.kind == DeviceKind::ImageFile),
        p.accent,
    );
    let tx = icon_rect.right() + 12.0;
    let max_w = rect.right() - tx - pad;
    let title = ui
        .painter()
        .layout(dev.model.clone(), theme::semibold(14.0), p.text, max_w);
    ui.painter()
        .galley(egui::pos2(tx, rect.top() + pad - 1.0), title, p.text);
    let kind = if dev.kind == DeviceKind::ImageFile {
        "Disk image".to_string()
    } else {
        format!("{} · {}", dev.bus.label(), dev.id)
    };
    ui.painter().text(
        egui::pos2(tx, rect.top() + pad + 20.0),
        egui::Align2::LEFT_TOP,
        format!("{} · {kind}", human_size(dev.size)),
        theme::mono(11.5),
        p.muted,
    );
    // Badges
    let mut bx = tx;
    let by = rect.top() + pad + 38.0;
    let mut badge = |text: &str, color: egui::Color32| {
        let g =
            ui.painter()
                .layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), color);
        let r = Rect::from_min_size(egui::pos2(bx, by), g.size() + Vec2::new(12.0, 4.0));
        ui.painter()
            .rect_filled(r, CornerRadius::same(4), Palette::soft(color, 36));
        ui.painter().galley(r.min + Vec2::new(6.0, 2.0), g, color);
        bx = r.right() + 6.0;
    };
    if dev.is_system {
        badge("System disk", p.warning);
    }
    if dev.removable {
        badge("Removable", p.audio);
    }
    if !dev.readable && dev.kind != DeviceKind::ImageFile {
        badge("Needs admin", p.danger);
    }
    if dev.rotational == Some(true) {
        badge("HDD", p.muted);
    } else if dev.rotational == Some(false) {
        badge("SSD", p.muted);
    }

    // Partition bar
    let bar = Rect::from_min_max(
        egui::pos2(rect.left() + pad, rect.bottom() - pad - 22.0),
        egui::pos2(rect.right() - pad, rect.bottom() - pad),
    );
    ui.painter()
        .rect_filled(bar, CornerRadius::same(5), p.map_unread);
    let size = dev.size.max(1) as f64;
    let mut clicked_part = None;
    for (i, part) in dev.partitions.iter().enumerate() {
        let x0 = bar.left() + (part.offset as f64 / size * bar.width() as f64) as f32;
        let x1 = bar.left() + ((part.offset + part.len) as f64 / size * bar.width() as f64) as f32;
        let seg = Rect::from_min_max(
            egui::pos2(x0.max(bar.left()) + 1.0, bar.top() + 1.0),
            egui::pos2(x1.min(bar.right()).max(x0 + 4.0) - 1.0, bar.bottom() - 1.0),
        );
        let id = resp.id.with(("part", i));
        let pr = ui.interact(seg, id, Sense::click());
        let hov = ui
            .ctx()
            .animate_bool_with_time(id, pr.hovered(), theme::ANIM);
        let is_sel = selected && selected_partition == Some(part.index);
        let base = fs_color(p, &part.fs);
        let col = Palette::soft(
            base,
            (110.0 + 80.0 * hov + if is_sel { 65.0 } else { 0.0 }) as u8,
        );
        ui.painter().rect_filled(seg, CornerRadius::same(4), col);
        if is_sel {
            ui.painter().rect_stroke(
                seg,
                CornerRadius::same(4),
                Stroke::new(1.5_f32, p.text),
                egui::StrokeKind::Inside,
            );
        }
        let label = match &part.label {
            Some(l) => format!("{} {l}", part.fs.label()),
            None => part.fs.label().to_string(),
        };
        if seg.width() > 48.0 {
            let g = ui.painter().layout(
                label.clone(),
                egui::FontId::proportional(11.0),
                p.text,
                seg.width() - 8.0,
            );
            let g_rect = Rect::from_center_size(seg.center(), g.size());
            if g.size().x <= seg.width() - 6.0 {
                ui.painter().galley(g_rect.min, g, p.text);
            }
        }
        let tip = format!(
            "{label}\n{} at {}{}\nClick to scan just this partition",
            human_size(part.len),
            human_size(part.offset),
            if part.mount_points.is_empty() {
                String::new()
            } else {
                format!("\nMounted at {}", part.mount_points.join(", "))
            }
        );
        if pr.on_hover_text(tip).clicked() {
            clicked_part = Some(part.index);
        }
    }
    if clicked_part.is_some() {
        out.partition = clicked_part;
        out.clicked = true;
    } else if resp.clicked() {
        out.clicked = true;
    }
    out
}
