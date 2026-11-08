//! Throughput sparkline (last 60 s).

use crate::theme::Palette;
use eframe::egui::{self, epaint::PathStroke, Color32, Pos2, Sense, Shape, Ui, Vec2};

pub fn sparkline(ui: &mut Ui, p: &Palette, values: &[f32], height: f32, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0_f32, p.border),
    );
    if values.len() < 2 {
        return;
    }
    let max = values.iter().cloned().fold(1.0f32, f32::max) * 1.15;
    let n = values.len();
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.left() + rect.width() * i as f32 / (n - 1) as f32,
                rect.bottom() - rect.height() * (v / max),
            )
        })
        .collect();
    // Filled area under the line.
    let mut mesh = egui::epaint::Mesh::default();
    let fill = Palette::soft(color, 40);
    let clear = Palette::soft(color, 0);
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(w[0], fill);
        mesh.colored_vertex(w[1], fill);
        mesh.colored_vertex(egui::pos2(w[1].x, rect.bottom()), clear);
        mesh.colored_vertex(egui::pos2(w[0].x, rect.bottom()), clear);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(pts, PathStroke::new(2.0_f32, color)));
}
