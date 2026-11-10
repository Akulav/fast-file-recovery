//! Async previews: thumbnails decoded off-thread (LRU of 200), text and hex of the first 4 KiB.

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use ffr_core::device::{open_reader, DeviceId, DeviceInfo, SourceHandle};
use ffr_core::recover::read_prefix;
use ffr_core::scan::{Hit, HitExtra, Session};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

const CACHE: usize = 200;
const HEX_BYTES: usize = 4096;
const IMAGE_MAX: u64 = 48 << 20;

struct Req {
    /// Session generation; results for an older session are dropped.
    generation: u64,
    id: u32,
    device: Arc<DeviceInfo>,
    hit: Hit,
    extra: Option<HitExtra>,
    ext: String,
}

struct Done {
    generation: u64,
    id: u32,
    image: Option<ColorImage>,
    text: Option<String>,
    hex: Vec<u8>,
    error: Option<String>,
}

pub struct Preview {
    pub texture: Option<TextureHandle>,
    pub text: Option<String>,
    pub hex: Vec<u8>,
    pub error: Option<String>,
}

pub struct PreviewManager {
    tx: Sender<Req>,
    rx: Receiver<Done>,
    cache: HashMap<u32, Preview>,
    order: VecDeque<u32>,
    pending: HashSet<u32>,
    device: Option<Arc<DeviceInfo>>,
    generation: u64,
}

/// Decode an image from disk bytes with tight limits; the bytes are untrusted.
fn decode_image(bytes: &[u8]) -> Result<ColorImage, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 << 20);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    reader.limits(limits);
    // A decoder bug on damaged data must not take the preview thread down with it.
    let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader.decode()))
        .map_err(|_| "decoder error".to_string())?
        .map_err(|e| e.to_string())?;
    let t = img.thumbnail(640, 640).to_rgba8();
    let size = [t.width() as usize, t.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, &t))
}

fn is_image(ext: &str) -> bool {
    matches!(
        ext,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "ico"
    )
}

fn is_text(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "xml"
            | "html"
            | "htm"
            | "rtf"
            | "csv"
            | "md"
            | "json"
            | "eml"
            | "log"
            | "ini"
            | "svg"
    )
}

fn worker(rx: Receiver<Req>, tx: Sender<Done>) {
    let mut handle: Option<(DeviceId, SourceHandle)> = None;
    while let Ok(req) = rx.recv() {
        if handle
            .as_ref()
            .map(|(id, _)| id != &req.device.id)
            .unwrap_or(true)
        {
            handle = open_reader(&req.device)
                .ok()
                .map(|h| (req.device.id.clone(), h));
        }
        let Some((_, src)) = &handle else {
            let _ = tx.send(Done {
                generation: req.generation,
                id: req.id,
                image: None,
                text: None,
                hex: Vec::new(),
                error: Some("Couldn't open the source for preview.".into()),
            });
            continue;
        };
        let mut done = Done {
            generation: req.generation,
            id: req.id,
            image: None,
            text: None,
            hex: Vec::new(),
            error: None,
        };
        let want_image = is_image(&req.ext) && req.hit.len <= IMAGE_MAX;
        let n = if want_image {
            req.hit.len as usize
        } else {
            HEX_BYTES
        };
        match read_prefix(src, &req.hit, req.extra.as_ref(), n) {
            Ok(bytes) => {
                done.hex = bytes[..bytes.len().min(HEX_BYTES)].to_vec();
                if want_image {
                    match decode_image(&bytes) {
                        Ok(img) => done.image = Some(img),
                        Err(e) => done.error = Some(format!("The image could not be decoded ({e}). It may be damaged or partly overwritten.")),
                    }
                }
                if is_text(&req.ext) {
                    done.text = Some(String::from_utf8_lossy(&done.hex).into_owned());
                }
            }
            Err(e) => done.error = Some(format!("Couldn't read the file: {e}")),
        }
        let _ = tx.send(done);
    }
}

impl Default for PreviewManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewManager {
    pub fn new() -> Self {
        let (tx, req_rx) = unbounded::<Req>();
        let (done_tx, rx) = unbounded::<Done>();
        for i in 0..2 {
            let r = req_rx.clone();
            let t = done_tx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("ffr-preview-{i}"))
                .spawn(move || worker(r, t));
        }
        Self {
            tx,
            rx,
            cache: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            device: None,
            generation: 0,
        }
    }

    /// Forget everything (new session).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
        self.pending.clear();
        self.device = None;
        self.generation += 1;
    }

    pub fn get(&self, id: u32) -> Option<&Preview> {
        self.cache.get(&id)
    }

    pub fn request(&mut self, id: u32, session: &Session) {
        if self.cache.contains_key(&id) || self.pending.contains(&id) {
            return;
        }
        let Some(hit) = session.hits.get(id as usize).copied() else {
            return;
        };
        let dev = match &self.device {
            Some(d) if d.id == session.source.device.id => d.clone(),
            _ => {
                let d = Arc::new(session.source.device.clone());
                self.device = Some(d.clone());
                d
            }
        };
        let ext = session.display_ext(&hit);
        self.pending.insert(id);
        let _ = self.tx.send(Req {
            generation: self.generation,
            id,
            device: dev,
            hit,
            extra: session.extra(&hit).cloned(),
            ext,
        });
    }

    /// Turn finished decodes into textures (UI thread).
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(d) = self.rx.try_recv() {
            if d.generation != self.generation {
                continue;
            }
            self.pending.remove(&d.id);
            let texture = d.image.map(|img| {
                ctx.load_texture(format!("preview-{}", d.id), img, TextureOptions::LINEAR)
            });
            self.cache.insert(
                d.id,
                Preview {
                    texture,
                    text: d.text,
                    hex: d.hex,
                    error: d.error,
                },
            );
            self.order.push_back(d.id);
            while self.order.len() > CACHE {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
            ctx.request_repaint();
        }
    }
}
