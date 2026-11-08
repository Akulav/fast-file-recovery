//! Large circular progress indicator.

use crate::theme::{self, Palette};
use eframe::egui::{self, epaint::PathStroke, Color32, Pos2, Sense, Shape, Stroke, Ui, Vec2};
use std::f32::consts::TAU;

fn arc(center: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = (((to - from).abs() / TAU) * 128.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            center + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// `fraction` in 0..=1; `None` draws an indeterminate spinner segment.
pub fn progress_ring(
    ui: &mut Ui,
    p: &Palette,
    size: f32,
    fraction: Option<f32>,
    big: &str,
    small: &str,
    color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 10.0;
    let width = 12.0;
    let painter = ui.painter();
    painter.circle_stroke(c, r, Stroke::new(width, p.map_unread));
    let start = -TAU / 4.0;
    match fraction {
        Some(f) => {
            let f = f.clamp(0.0, 1.0);
            if f > 0.0005 {
                let pts = arc(c, r, start, start + TAU * f);
                painter.add(Shape::line(pts.clone(), PathStroke::new(width, color)));
                // Round caps.
                painter.circle_filled(pts[0], width / 2.0, color);
                if let Some(last) = pts.last() {
                    painter.circle_filled(*last, width / 2.0, color);
                    painter.circle_filled(*last, width / 2.0 + 4.0, Palette::soft(color, 40));
                }
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let a = start + t * 3.0;
            let pts = arc(c, r, a, a + TAU * 0.22);
            painter.add(Shape::line(pts, PathStroke::new(width, color)));
            ui.ctx().request_repaint();
        }
    }
    painter.text(
        c - Vec2::new(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        big,
        theme::semibold(size * 0.17),
        p.text,
    );
    painter.text(
        c + Vec2::new(0.0, size * 0.12),
        egui::Align2::CENTER_CENTER,
        small,
        theme::mono(12.0),
        p.muted,
    );
}
