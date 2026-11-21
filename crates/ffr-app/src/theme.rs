//! Design system: palette, fonts, egui style and spacing tokens.
//! All colours live here; there are no raw `Color32` literals elsewhere.
//!
//! Both themes are Windows 11's, like Akulav's other apps (Hyve): a flat base ([`paint_backdrop`]), card layers with
//! hairline borders ([`Palette::glass`]), the Windows accent blue, and for the current page a faint layer with the
//! accent pill ([`paint_active_pill`]). Segoe UI on Windows, Inter elsewhere.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect,
    Shadow, Stroke, TextStyle, Visuals,
};
use ffr_core::kinds::Category;
use std::sync::Arc;

/// Spacing grid and shapes.
pub const SPACE: f32 = 8.0;
pub const CARD_RADIUS: u8 = 8;
pub const CONTROL_RADIUS: u8 = 4;
/// Sidebar (navigation pane) width, including its gap to the window edge.
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
        if self.dark {
            Self::soft(Color32::WHITE, 15)
        } else {
            Self::soft(Color32::BLACK, 10)
        }
    }
    /// Inset fill for text fields and tracks.
    pub fn inset(&self) -> Color32 {
        if self.dark {
            hex(0x1C1C1C)
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
    /// Panel and card fill: Windows 11's card layer, one step above the window.
    pub fn glass(&self) -> Color32 {
        self.surface
    }
    /// Hairline border of panels and cards.
    pub fn glass_border(&self) -> Color32 {
        self.border
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
            bg: hex(0x202020),
            surface: hex(0x2B2B2B),
            raised: hex(0x323232),
            border: hex(0x3A3A3A),
            text: hex(0xFFFFFF),
            muted: hex(0xCFCFCF),
            accent: hex(0x60CDFF),
            accent_hover: hex(0x5ABCE9),
            on_accent: hex(0x000000),
            success: hex(0x6CCB5F),
            warning: hex(0xFCE100),
            danger: hex(0xFF99A4),
            images: hex(0x60CDFF),
            video: hex(0xB47CFF),
            audio: hex(0x2BC4A9),
            documents: hex(0xF2A93B),
            archives: hex(0xE86E5A),
            other: hex(0x8A91A0),
            shadow: Color32::from_black_alpha(90),
            map_unread: hex(0x383838),
        }
    }

    /// Windows 11 light: the same roles, ≥ 4.5:1 contrast on `#F3F3F3`.
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xF3F3F3),
            surface: hex(0xFBFBFB),
            raised: hex(0xFFFFFF),
            border: hex(0xE5E5E5),
            text: hex(0x1A1A1A),
            muted: hex(0x5D5D5D),
            accent: hex(0x005FB8),
            accent_hover: hex(0x196EBF),
            on_accent: hex(0xFFFFFF),
            success: hex(0x0F7B0F),
            warning: hex(0x9D5D00),
            danger: hex(0xC42B1C),
            images: hex(0x005FB8),
            video: hex(0x7E3FD1),
            audio: hex(0x0C7D6B),
            documents: hex(0x9A5B00),
            archives: hex(0xB93A26),
            other: hex(0x5C6370),
            shadow: Color32::from_black_alpha(28),
            map_unread: hex(0xE5E5E5),
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

/// The window background: Windows 11's flat base.
pub fn paint_backdrop(ctx: &egui::Context, p: &Palette) {
    let r: Rect = ctx.screen_rect();
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(r, CornerRadius::ZERO, p.bg);
}

/// The current page's nav item, as in Windows 11: a faint layer and a short accent pill on the left edge.
/// `a` fades it in (0..=1).
pub fn paint_active_pill(painter: &egui::Painter, rect: Rect, p: &Palette, a: f32) {
    painter.rect_filled(
        rect,
        CornerRadius::same(CONTROL_RADIUS),
        Palette::soft(p.hover_fill(), (p.hover_fill().a() as f32 * a) as u8),
    );
    let pill = Rect::from_center_size(
        egui::pos2(rect.left() + 1.5, rect.center().y),
        egui::vec2(3.0, 16.0 * a),
    );
    painter.rect_filled(pill, CornerRadius::same(2), p.accent);
}

/// A font from the Windows fonts folder: Windows 11's Segoe UI can be used but not shipped.
fn system_font(file: &str) -> Option<Vec<u8>> {
    if !cfg!(windows) {
        return None;
    }
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    std::fs::read(std::path::Path::new(&windir).join("Fonts").join(file)).ok()
}

pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    // Segoe UI on Windows (it has no Private Use Area glyphs), the bundled Inter everywhere else.
    let pick = |system: &str, bundled: &'static [u8]| match system_font(system) {
        Some(bytes) => FontData::from_owned(bytes),
        None => FontData::from_static(bundled),
    };
    f.font_data.insert(
        "inter".into(),
        Arc::new(pick(
            "segoeui.ttf",
            include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
        )),
    );
    f.font_data.insert(
        "inter-semibold".into(),
        Arc::new(pick(
            "seguisb.ttf",
            include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
        )),
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
    v.selection.bg_fill = Palette::soft(p.accent, 90);
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
    w.hovered.bg_fill = Palette::mix(p.raised, p.text, 0.05);
    w.hovered.weak_bg_fill = Palette::mix(p.raised, p.text, 0.05);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::mix(p.border, p.text, 0.12));
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = Palette::mix(p.raised, p.text, 0.08);
    w.active.weak_bg_fill = Palette::mix(p.raised, p.text, 0.08);
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
