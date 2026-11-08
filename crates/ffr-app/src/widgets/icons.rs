//! Embedded Lucide-style SVG icons, rasterised by `egui_extras` (crisp at any DPI) and tinted.

use eframe::egui::{self, include_image, Color32, ImageSource, Rect, Response, Ui, Vec2};
use ffr_core::device::BusType;
use ffr_core::kinds::Category;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    HardDrive,
    Usb,
    SdCard,
    Disc,
    Image,
    Film,
    Music,
    FileText,
    Archive,
    Box,
    Search,
    Settings,
    Info,
    Play,
    Pause,
    Stop,
    Refresh,
    Check,
    X,
    Lock,
    Unlock,
    Download,
    Alert,
    Scan,
    List,
    Recover,
    Folder,
    Save,
    Shield,
    Copy,
    ArrowRight,
    Cpu,
}

impl Icon {
    pub fn source(self) -> ImageSource<'static> {
        match self {
            Icon::HardDrive => include_image!("../../../../assets/ui-icons/hard-drive.svg"),
            Icon::Usb => include_image!("../../../../assets/ui-icons/usb.svg"),
            Icon::SdCard => include_image!("../../../../assets/ui-icons/sd-card.svg"),
            Icon::Disc => include_image!("../../../../assets/ui-icons/disc.svg"),
            Icon::Image => include_image!("../../../../assets/ui-icons/image.svg"),
            Icon::Film => include_image!("../../../../assets/ui-icons/film.svg"),
            Icon::Music => include_image!("../../../../assets/ui-icons/music.svg"),
            Icon::FileText => include_image!("../../../../assets/ui-icons/file-text.svg"),
            Icon::Archive => include_image!("../../../../assets/ui-icons/archive.svg"),
            Icon::Box => include_image!("../../../../assets/ui-icons/box.svg"),
            Icon::Search => include_image!("../../../../assets/ui-icons/search.svg"),
            Icon::Settings => include_image!("../../../../assets/ui-icons/settings.svg"),
            Icon::Info => include_image!("../../../../assets/ui-icons/info.svg"),
            Icon::Play => include_image!("../../../../assets/ui-icons/play.svg"),
            Icon::Pause => include_image!("../../../../assets/ui-icons/pause.svg"),
            Icon::Stop => include_image!("../../../../assets/ui-icons/square.svg"),
            Icon::Refresh => include_image!("../../../../assets/ui-icons/refresh.svg"),
            Icon::Check => include_image!("../../../../assets/ui-icons/check.svg"),
            Icon::X => include_image!("../../../../assets/ui-icons/x.svg"),
            Icon::Lock => include_image!("../../../../assets/ui-icons/lock.svg"),
            Icon::Unlock => include_image!("../../../../assets/ui-icons/unlock.svg"),
            Icon::Download => include_image!("../../../../assets/ui-icons/download.svg"),
            Icon::Alert => include_image!("../../../../assets/ui-icons/alert.svg"),
            Icon::Scan => include_image!("../../../../assets/ui-icons/scan.svg"),
            Icon::List => include_image!("../../../../assets/ui-icons/list.svg"),
            Icon::Recover => include_image!("../../../../assets/ui-icons/recover.svg"),
            Icon::Folder => include_image!("../../../../assets/ui-icons/folder.svg"),
            Icon::Save => include_image!("../../../../assets/ui-icons/save.svg"),
            Icon::Shield => include_image!("../../../../assets/ui-icons/shield.svg"),
            Icon::Copy => include_image!("../../../../assets/ui-icons/copy.svg"),
            Icon::ArrowRight => include_image!("../../../../assets/ui-icons/arrow-right.svg"),
            Icon::Cpu => include_image!("../../../../assets/ui-icons/cpu.svg"),
        }
    }

    pub fn for_category(c: Category) -> Icon {
        match c {
            Category::Images => Icon::Image,
            Category::Video => Icon::Film,
            Category::Audio => Icon::Music,
            Category::Documents => Icon::FileText,
            Category::Archives => Icon::Archive,
            Category::Other => Icon::Box,
        }
    }

    pub fn for_bus(b: BusType, image_file: bool) -> Icon {
        if image_file {
            return Icon::Disc;
        }
        match b {
            BusType::Usb => Icon::Usb,
            BusType::Sd => Icon::SdCard,
            BusType::Nvme => Icon::Cpu,
            _ => Icon::HardDrive,
        }
    }
}

/// Add an icon as a widget.
pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    ui.add(
        egui::Image::new(icon.source())
            .fit_to_exact_size(Vec2::splat(size))
            .tint(color),
    )
}

/// Paint an icon into a rect.
pub fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    egui::Image::new(icon.source())
        .tint(color)
        .fit_to_exact_size(rect.size())
        .paint_at(ui, rect);
}
