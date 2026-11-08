//! Icon and installer-image rendering from `assets/icon.svg`.

use crate::Result;
use resvg::{tiny_skia, usvg};
use std::path::Path;

fn tree_from(svg: &str) -> Result<usvg::Tree> {
    usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| format!("svg: {e}"))
}

fn render(tree: &usvg::Tree, w: u32, h: u32) -> Result<tiny_skia::Pixmap> {
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or("bad size")?;
    let size = tree.size();
    let t = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(tree, t, &mut pm.as_mut());
    Ok(pm)
}

/// Demultiplied RGBA bytes.
fn rgba(pm: &tiny_skia::Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

pub fn icons(root: &Path) -> Result<()> {
    let svg = std::fs::read_to_string(root.join("assets/icon.svg")).map_err(|e| e.to_string())?;
    let tree = tree_from(&svg)?;
    let out = root.join("assets/icons");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let sizes = [16u32, 24, 32, 48, 64, 128, 256, 512, 1024];
    let mut ico = ico::IconDir::new(ico::ResourceType::Icon);
    let mut icns = icns::IconFamily::new();
    for &s in &sizes {
        let pm = render(&tree, s, s)?;
        let data = rgba(&pm);
        let img = image::RgbaImage::from_raw(s, s, data.clone()).ok_or("buffer")?;
        img.save(out.join(format!("icon-{s}.png")))
            .map_err(|e| e.to_string())?;
        if s <= 256 {
            let ii = ico::IconImage::from_rgba_data(s, s, data.clone());
            ico.add_entry(ico::IconDirEntry::encode(&ii).map_err(|e| e.to_string())?);
        }
        let ty = match s {
            16 => Some(icns::IconType::RGBA32_16x16),
            32 => Some(icns::IconType::RGBA32_32x32),
            64 => Some(icns::IconType::RGBA32_64x64),
            128 => Some(icns::IconType::RGBA32_128x128),
            256 => Some(icns::IconType::RGBA32_256x256),
            512 => Some(icns::IconType::RGBA32_512x512),
            1024 => Some(icns::IconType::RGBA32_512x512_2x),
            _ => None,
        };
        if let Some(ty) = ty {
            let im = icns::Image::from_data(icns::PixelFormat::RGBA, s, s, data)
                .map_err(|e| e.to_string())?;
            icns.add_icon_with_type(&im, ty)
                .map_err(|e| e.to_string())?;
        }
    }
    let f = std::fs::File::create(out.join("icon.ico")).map_err(|e| e.to_string())?;
    ico.write(f).map_err(|e| e.to_string())?;
    let f = std::fs::File::create(out.join("icon.icns")).map_err(|e| e.to_string())?;
    icns.write(f).map_err(|e| e.to_string())?;
    println!("wrote assets/icons (png 16–1024, icon.ico, icon.icns)");
    Ok(())
}

fn inner_svg(svg: &str) -> Result<String> {
    let open_end = svg
        .find("<svg")
        .and_then(|i| svg[i..].find('>').map(|j| i + j + 1));
    let close = svg.rfind("</svg>");
    match (open_end, close) {
        (Some(a), Some(b)) if a < b => Ok(svg[a..b].to_string()),
        _ => Err("icon.svg is not a plain <svg> document".into()),
    }
}

fn save_bmp(pm: &tiny_skia::Pixmap, path: &Path) -> Result<()> {
    let data = rgba(pm);
    let rgb: Vec<u8> = data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    let img = image::RgbImage::from_raw(pm.width(), pm.height(), rgb).ok_or("buffer")?;
    img.save_with_format(path, image::ImageFormat::Bmp)
        .map_err(|e| e.to_string())
}

/// Inno Setup wizard images, drawn at 2× (Inno scales them for the current DPI).
pub fn installer_images(root: &Path) -> Result<()> {
    let icon = std::fs::read_to_string(root.join("assets/icon.svg")).map_err(|e| e.to_string())?;
    let inner = inner_svg(&icon)?;
    let large = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="328" height="628" viewBox="0 0 328 628">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="0.4" y2="1">
      <stop offset="0" stop-color="#171A21"/><stop offset="1" stop-color="#0F1115"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.32" r="0.55">
      <stop offset="0" stop-color="#4F8CFF" stop-opacity="0.35"/><stop offset="1" stop-color="#4F8CFF" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <rect width="328" height="628" fill="url(#g)"/>
  <rect width="328" height="628" fill="url(#glow)"/>
  <g stroke="#2A2F3A" stroke-width="2" fill="none" opacity="0.8">
    <circle cx="164" cy="200" r="130"/><circle cx="164" cy="200" r="170"/><circle cx="164" cy="200" r="210"/>
  </g>
  <svg x="84" y="120" width="160" height="160" viewBox="0 0 256 256">{inner}</svg>
  <g fill="#4F8CFF"><rect x="104" y="400" width="120" height="6" rx="3"/></g>
  <g fill="#2A2F3A"><rect x="124" y="420" width="80" height="6" rx="3"/><rect x="140" y="440" width="48" height="6" rx="3"/></g>
</svg>"##
    );
    let small = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="110" height="116" viewBox="0 0 110 116">
  <rect width="110" height="116" fill="#FFFFFF"/>
  <svg x="7" y="10" width="96" height="96" viewBox="0 0 256 256">{inner}</svg>
</svg>"##
    );
    let out = root.join("installer");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    save_bmp(
        &render(&tree_from(&large)?, 328, 628)?,
        &out.join("wizard-large.bmp"),
    )?;
    save_bmp(
        &render(&tree_from(&small)?, 110, 116)?,
        &out.join("wizard-small.bmp"),
    )?;
    println!("wrote installer/wizard-large.bmp and wizard-small.bmp");
    Ok(())
}
