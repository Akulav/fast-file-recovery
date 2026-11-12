//! Design system: palette, fonts, egui style and spacing tokens.
//! All colours live here; there are no raw `Color32` literals elsewhere.
//!
//! The dark theme follows Akulav's newer apps (Hyve, Lumen): a deep blue-black backdrop with soft glows
//! ([`paint_backdrop`]), glass panels with hairline borders ([`Palette::glass`]) and a gradient pill for the
//! current page ([`paint_active_pill`]).

use eframe::egui::{
    self,
    epaint::{Mesh, Vertex, WHITE_UV},
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Pos2, Rect,
    Shadow, Stroke, TextStyle, Visuals,
};
use ffr_core::kinds::Category;
use std::sync::Arc;

/// Spacing grid and shapes.
pub const SPACE: f32 = 8.0;
pub const CARD_RADIUS: u8 = 14;
pub const CONTROL_RADIUS: u8 = 8;
/// Sidebar panel width, including the gap around the floating glass panel.
pub const SIDEBAR_WIDTH: f32 = 250.0;
/// Gap between the window edge, the sidebar panel and the content.
pub const WINDOW_GAP: f32 = 14.0;
pub const ANIM: f32 = 0.15;

pub const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub images: Color32,
    pub video: Color32,
    pub audio: Color32,
    pub documents: Color32,
    pub archives: Color32,
    pub other: Color32,
    pub shadow: Color32,
    /// Sector map: unread cells.
    pub map_unread: Color32,
}

impl Palette {
    /// Hairline used for dividers inside cards and panels (quieter than `border`).
    pub fn divider(&self) -> Color32 {
        Self::mix(self.border, self.bg, 0.35)
    }
    /// Fill of a hovered row or item on a surface.
    pub fn hover_fill(&self) -> Color32 {
        let base = if self.dark { self.raised } else { self.bg };
        Self::mix(base, self.accent, 0.05)
    }
    /// Inset fill for text fields and tracks.
    pub fn inset(&self) -> Color32 {
        if self.dark {
            Self::mix(self.bg, Color32::BLACK, 0.18)
        } else {
            Self::mix(self.bg, self.border, 0.25)
        }
    }
    /// Keyboard focus ring.
    pub fn focus(&self) -> Color32 {
        Self::soft(self.accent, 150)
    }
    /// Soft layered shadow for raised elements.
    pub fn soft_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 6],
            blur: 22,
            spread: 0,
            color: self.shadow,
        }
    }
    /// Panel and card fill: translucent white over the dark backdrop (glass), plain surface in the light theme.
    pub fn glass(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_premultiplied(14, 14, 15, 14)
        } else {
            self.surface
        }
    }
    /// Hairline border of glass panels and cards.
    pub fn glass_border(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_premultiplied(24, 24, 26, 24)
        } else {
            self.border
        }
    }
    /// Very light shadow for resting cards (barely visible, adds depth).
    pub fn rest_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 1],
            blur: 3,
            spread: 0,
            color: Self::soft(Color32::BLACK, if self.dark { 50 } else { 12 }),
        }
    }
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x090B14),
            surface: hex(0x10131F),
            raised: hex(0x171B2A),
            border: hex(0x2A2F45),
            text: hex(0xE8EAF6),
            muted: hex(0x8F95B2),
            accent: hex(0x4F8CFF),
            accent_hover: hex(0x6A9EFF),
            on_accent: hex(0xFFFFFF),
            success: hex(0x34D399),
            warning: hex(0xFBBF24),
            danger: hex(0xF87171),
            images: hex(0x4F8CFF),
            video: hex(0xB47CFF),
            audio: hex(0x2BC4A9),
            documents: hex(0xF2A93B),
            archives: hex(0xE86E5A),
            other: hex(0x8A91A0),
            shadow: Color32::from_black_alpha(90),
            map_unread: hex(0x1C2132),
        }
    }

    /// Same hues, adjusted for ≥ 4.5:1 contrast on `#F7F8FA`.
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xF7F8FA),
            surface: hex(0xFFFFFF),
            raised: hex(0xFFFFFF),
            border: hex(0xDDE1E8),
            text: hex(0x1A1D24),
            muted: hex(0x5C6370),
            accent: hex(0x1F5FD6),
            accent_hover: hex(0x2F6FE6),
            on_accent: hex(0xFFFFFF),
            success: hex(0x1A7F37),
            warning: hex(0x9A6700),
            danger: hex(0xCF222E),
            images: hex(0x1F5FD6),
            video: hex(0x7E3FD1),
            audio: hex(0x0C7D6B),
            documents: hex(0x9A5B00),
            archives: hex(0xB93A26),
            other: hex(0x5C6370),
            shadow: Color32::from_black_alpha(28),
            map_unread: hex(0xE6E9EF),
        }
    }

    pub fn category(&self, c: Category) -> Color32 {
        match c {
            Category::Images => self.images,
            Category::Video => self.video,
            Category::Audio => self.audio,
            Category::Documents => self.documents,
            Category::Archives => self.archives,
            Category::Other => self.other,
        }
    }

    /// A colour with reduced opacity (for soft fills behind icons and chips).
    pub fn soft(c: Color32, alpha: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
    }

    pub fn transparent() -> Color32 {
        Color32::TRANSPARENT
    }

    pub fn white() -> Color32 {
        Color32::WHITE
    }

    /// Mix two colours (t = 0 → a, 1 → b).
    pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgba_unmultiplied(
            l(a.r(), b.r()),
            l(a.g(), b.g()),
            l(a.b(), b.b()),
            l(a.a(), b.a()),
        )
    }

    /// Contrast ratio (WCAG) between two colours.
    #[cfg(test)]
    pub fn contrast(a: Color32, b: Color32) -> f32 {
        fn lum(c: Color32) -> f32 {
            let ch = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
}

/// Soft radial glow: `color` in the middle, fading to nothing at `radius`.
fn radial(mesh: &mut Mesh, center: Pos2, radius: egui::Vec2, color: Color32) {
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color,
    });
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let pos = center + egui::vec2(a.cos() * radius.x, a.sin() * radius.y);
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: Color32::TRANSPARENT,
        });
    }
    for i in 0..n {
        mesh.add_triangle(base, base + 1 + i, base + 2 + i);
    }
}

/// The window backdrop: the base colour with an accent glow top-left, a teal glow top-right and a violet one below.
pub fn paint_backdrop(ctx: &egui::Context, p: &Palette) {
    let r: Rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(r, CornerRadius::ZERO, p.bg);
    let k = if p.dark { 1.0 } else { 0.35 };
    let (w, h) = (r.width(), r.height());
    let mut mesh = Mesh::default();
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 0.08, -h * 0.10),
        egui::vec2(w * 0.62, h * 0.85),
        p.accent.gamma_multiply(0.16 * k),
    );
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 1.05, h * 0.10),
        egui::vec2(w * 0.48, h * 0.75),
        p.audio.gamma_multiply(0.10 * k),
    );
    radial(
        &mut mesh,
        r.min + egui::vec2(w * 0.5, h * 1.2),
        egui::vec2(w * 0.45, h * 0.6),
        p.video.gamma_multiply(0.08 * k),
    );
    painter.add(mesh);
}

/// The current page's nav item: an accent gradient fading to the right with an accent border (Hyve's nav).
/// `a` fades it in (0..=1).
pub fn paint_active_pill(painter: &egui::Painter, rect: Rect, p: &Palette, a: f32) {
    let radius = CONTROL_RADIUS + 2;
    let r = radius as f32;
    let left = Palette::soft(p.accent, (72.0 * a) as u8);
    let right = Palette::soft(p.accent, (20.0 * a) as u8);
    // Caps clipped to their own strips so the translucent colours never overlap.
    let lstrip = Rect::from_min_max(rect.min, egui::pos2(rect.min.x + r, rect.max.y));
    let rstrip = Rect::from_min_max(egui::pos2(rect.max.x - r, rect.min.y), rect.max);
    painter
        .with_clip_rect(lstrip.intersect(painter.clip_rect()))
        .rect_filled(
            Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0 * r, rect.max.y)),
            CornerRadius {
                nw: radius,
                sw: radius,
                ne: 0,
                se: 0,
            },
            left,
        );
    painter
        .with_clip_rect(rstrip.intersect(painter.clip_rect()))
        .rect_filled(
            Rect::from_min_max(egui::pos2(rect.max.x - 2.0 * r, rect.min.y), rect.max),
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: radius,
                se: radius,
            },
            right,
        );
    let mid = Rect::from_min_max(
        egui::pos2(rect.min.x + r, rect.min.y),
        egui::pos2(rect.max.x - r, rect.max.y),
    );
    let mut mesh = Mesh::default();
    for (pos, c) in [
        (mid.left_top(), left),
        (mid.right_top(), right),
        (mid.right_bottom(), right),
        (mid.left_bottom(), left),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: c,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0_f32, Palette::soft(p.accent, (90.0 * a) as u8)),
        egui::StrokeKind::Inside,
    );
}

pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    f.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    f.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    f.font_data.insert(
        "jbmono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    f.families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    f.families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jbmono".into());
    let mut semi = vec!["inter-semibold".to_string()];
    semi.extend(
        f.families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    f.families.insert(FontFamily::Name(SEMIBOLD.into()), semi);
    f
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Install fonts once.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
}

/// Apply a palette to the egui style.
pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.override_text_color = Some(p.text);
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = Palette::soft(p.accent, 70);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.accent);
    v.extreme_bg_color = p.inset();
    v.faint_bg_color = Palette::mix(p.bg, p.surface, 0.55);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(CONTROL_RADIUS);
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_shadow = p.soft_shadow();
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: p.shadow,
    };
    v.striped = true;
    let r = CornerRadius::same(CONTROL_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    // Separators and frames drawn by egui itself use this stroke: keep it quiet.
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.divider());
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = Palette::mix(p.raised, p.accent, 0.12);
    w.hovered.weak_bg_fill = Palette::mix(p.raised, p.accent, 0.12);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::mix(p.border, p.accent, 0.45));
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = Palette::mix(p.raised, p.accent, 0.22);
    w.active.weak_bg_fill = Palette::mix(p.raised, p.accent, 0.22);
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open = w.active;

    let mut style = (*ctx.style()).clone();
    style.visuals = v;
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
        (TextStyle::Heading, semibold(22.0)),
        (TextStyle::Name("section".into()), semibold(15.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(SPACE, SPACE * 0.75);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.icon_width = 16.0;
    style.spacing.icon_width_inner = 9.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.combo_height = 280.0;
    style.spacing.text_edit_width = 220.0;
    style.spacing.window_margin = Margin::same(16);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.animation_time = ANIM;
    style.interaction.tooltip_delay = 0.35;
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_palette_meets_contrast() {
        let p = Palette::light();
        for c in [p.text, p.muted, p.accent, p.success, p.warning, p.danger] {
            assert!(Palette::contrast(c, p.bg) >= 4.5, "{c:?}");
        }
        for cat in Category::ALL {
            assert!(Palette::contrast(p.category(cat), p.bg) >= 4.5, "{cat:?}");
        }
    }

    #[test]
    fn dark_text_contrast() {
        let p = Palette::dark();
        assert!(Palette::contrast(p.text, p.bg) >= 7.0);
        assert!(Palette::contrast(p.muted, p.bg) >= 4.5);
    }
}
