//! Design system: palette, fonts, egui style and spacing tokens.
//! All colours live here; there are no raw `Color32` literals elsewhere.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};
use ffr_core::kinds::Category;
use std::sync::Arc;

/// Spacing grid and shapes.
pub const SPACE: f32 = 8.0;
pub const CARD_RADIUS: u8 = 10;
pub const CONTROL_RADIUS: u8 = 6;
pub const SIDEBAR_WIDTH: f32 = 220.0;
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

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x0F1115),
            surface: hex(0x171A21),
            raised: hex(0x1E222B),
            border: hex(0x2A2F3A),
            text: hex(0xE6E8EE),
            muted: hex(0x8A91A0),
            accent: hex(0x4F8CFF),
            accent_hover: hex(0x6A9EFF),
            on_accent: hex(0xFFFFFF),
            success: hex(0x3FB950),
            warning: hex(0xD29922),
            danger: hex(0xF85149),
            images: hex(0x4F8CFF),
            video: hex(0xB47CFF),
            audio: hex(0x2BC4A9),
            documents: hex(0xF2A93B),
            archives: hex(0xE86E5A),
            other: hex(0x8A91A0),
            shadow: Color32::from_black_alpha(90),
            map_unread: hex(0x232833),
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
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(CONTROL_RADIUS);
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: p.shadow,
    };
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
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = Palette::mix(p.raised, p.accent, 0.12);
    w.hovered.weak_bg_fill = Palette::mix(p.raised, p.accent, 0.12);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::mix(p.border, p.accent, 0.5));
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
