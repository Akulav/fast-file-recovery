//! Toasts: bottom right, auto-hide after 4 s; errors stay until closed.

use super::icons::{self, Icon};
use crate::theme::{self, Palette};
use eframe::egui::{self, Align2, CornerRadius, Frame, Margin, RichText, Stroke};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

struct Toast {
    id: u64,
    kind: Kind,
    text: String,
    created: Instant,
}

#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Toasts {
    pub fn push(&mut self, kind: Kind, text: impl Into<String>) {
        self.next += 1;
        self.list.push(Toast {
            id: self.next,
            kind,
            text: text.into(),
            created: Instant::now(),
        });
        if self.list.len() > 5 {
            self.list.remove(0);
        }
    }
    pub fn info(&mut self, t: impl Into<String>) {
        self.push(Kind::Info, t);
    }
    pub fn success(&mut self, t: impl Into<String>) {
        self.push(Kind::Success, t);
    }
    pub fn warning(&mut self, t: impl Into<String>) {
        self.push(Kind::Warning, t);
    }
    pub fn error(&mut self, t: impl Into<String>) {
        self.push(Kind::Error, t);
    }

    pub fn show(&mut self, ctx: &egui::Context, p: &Palette) {
        let life = Duration::from_secs(4);
        self.list
            .retain(|t| t.kind == Kind::Error || t.created.elapsed() < life);
        let mut close = None;
        let mut y = -16.0;
        for t in self.list.iter().rev() {
            let (color, icon) = match t.kind {
                Kind::Info => (p.accent, Icon::Info),
                Kind::Success => (p.success, Icon::Check),
                Kind::Warning => (p.warning, Icon::Alert),
                Kind::Error => (p.danger, Icon::Alert),
            };
            let age = t.created.elapsed().as_secs_f32();
            let fade_in = (age / theme::ANIM).min(1.0);
            let fade_out = if t.kind == Kind::Error {
                1.0
            } else {
                ((life.as_secs_f32() - age) / 0.3).clamp(0.0, 1.0)
            };
            let alpha = fade_in.min(fade_out);
            let area = egui::Area::new(egui::Id::new(("toast", t.id)))
                .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, y))
                .order(egui::Order::Foreground)
                .interactable(true);
            let r = area.show(ctx, |ui| {
                ui.set_opacity(alpha);
                // Slide up slightly while fading in.
                let lift = (1.0 - fade_in) * 8.0;
                if lift > 0.0 {
                    ui.add_space(lift);
                }
                let fr = Frame::new()
                    .fill(p.raised)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
                    .inner_margin(Margin {
                        left: 18,
                        right: 12,
                        top: 11,
                        bottom: 11,
                    })
                    .shadow(p.soft_shadow())
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.horizontal(|ui| {
                            let (ir, _) = ui
                                .allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                            ui.painter()
                                .circle_filled(ir.center(), 12.0, Palette::soft(color, 38));
                            icons::paint(ui, ir.shrink(5.0), icon, color);
                            ui.add(egui::Label::new(RichText::new(&t.text).color(p.text)).wrap());
                            if t.kind == Kind::Error {
                                let (cr, cresp) = ui.allocate_exact_size(
                                    egui::vec2(20.0, 20.0),
                                    egui::Sense::click(),
                                );
                                let h = ui.ctx().animate_bool_with_time(
                                    cresp.id,
                                    cresp.hovered(),
                                    theme::ANIM,
                                );
                                ui.painter().rect_filled(
                                    cr,
                                    CornerRadius::same(theme::CONTROL_RADIUS),
                                    Palette::soft(p.muted, (40.0 * h) as u8),
                                );
                                icons::paint(
                                    ui,
                                    cr.shrink(4.0),
                                    Icon::X,
                                    Palette::mix(p.muted, p.text, h),
                                );
                                if cresp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("Dismiss")
                                    .clicked()
                                {
                                    close = Some(t.id);
                                }
                            }
                        });
                    });
                // Coloured stripe on the leading edge.
                let rect = fr.response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.left() + 3.0, rect.bottom()),
                    ),
                    CornerRadius {
                        nw: theme::CARD_RADIUS,
                        sw: theme::CARD_RADIUS,
                        ne: 0,
                        se: 0,
                    },
                    color,
                );
            });
            y -= r.response.rect.height() + 8.0;
            if fade_in < 1.0 || (t.kind != Kind::Error && fade_out < 1.0) {
                ctx.request_repaint();
            }
        }
        if let Some(id) = close {
            self.list.retain(|t| t.id != id);
        }
        if !self.list.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}
