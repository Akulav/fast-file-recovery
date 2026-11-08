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
                Frame::new()
                    .fill(p.raised)
                    .stroke(Stroke::new(1.0_f32, Palette::soft(color, 140)))
                    .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
                    .inner_margin(Margin::symmetric(14, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 18,
                        spread: 0,
                        color: p.shadow,
                    })
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.horizontal(|ui| {
                            icons::icon(ui, icon, 16.0, color);
                            ui.add(egui::Label::new(RichText::new(&t.text).color(p.text)).wrap());
                            if t.kind == Kind::Error
                                && ui
                                    .add(
                                        egui::Button::new(RichText::new("✕").color(p.muted))
                                            .frame(false),
                                    )
                                    .clicked()
                            {
                                close = Some(t.id);
                            }
                        });
                    });
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
