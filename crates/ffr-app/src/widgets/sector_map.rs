//! Sector map: ~4,000 cells, each 1/4000 of the disk (unread / read / hits / bad).

use crate::theme::Palette;
use eframe::egui::{self, epaint::Mesh, Rect, Sense, Shape, Ui, Vec2};

pub fn sector_map(ui: &mut Ui, p: &Palette, cells: &[u8], height: f32) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    if !ui.is_rect_visible(rect) || cells.is_empty() {
        return;
    }
    // Choose columns so cells are close to square.
    let n = cells.len() as f32;
    let cols = ((n * width / height).sqrt()).round().max(1.0) as usize;
    let rows = cells.len().div_ceil(cols);
    let cw = width / cols as f32;
    let ch = height / rows as f32;
    let gap = if cw > 4.0 && ch > 4.0 { 1.0 } else { 0.0 };
    let mut mesh = Mesh::default();
    for (i, &c) in cells.iter().enumerate() {
        let x = rect.left() + (i % cols) as f32 * cw;
        let y = rect.top() + (i / cols) as f32 * ch;
        let color = match c {
            1 => Palette::soft(p.accent, 120),
            2 => p.success,
            3 => p.danger,
            _ => p.map_unread,
        };
        let r = Rect::from_min_size(egui::pos2(x, y), Vec2::new(cw - gap, ch - gap));
        mesh.add_colored_rect(r, color);
    }
    ui.painter().add(Shape::mesh(mesh));
    if let Some(pos) = resp.hover_pos() {
        let col = ((pos.x - rect.left()) / cw) as usize;
        let row = ((pos.y - rect.top()) / ch) as usize;
        let i = row * cols + col.min(cols - 1);
        if let Some(&c) = cells.get(i) {
            let state = match c {
                1 => "read",
                2 => "contains found files",
                3 => "unreadable sectors",
                _ => "not read yet",
            };
            let pct = i as f32 / cells.len() as f32 * 100.0;
            resp.on_hover_text(format!("{pct:.1}% into the disk: {state}"));
        }
    }
}
