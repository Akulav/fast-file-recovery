//! Hex view of the first bytes of a file.

use crate::theme::{self, Palette};
use eframe::egui::{self, text::LayoutJob, TextFormat, Ui};

pub fn hex_view(ui: &mut Ui, p: &Palette, bytes: &[u8], base: u64) {
    let font = theme::mono(11.5);
    egui::ScrollArea::vertical()
        .id_salt("hex")
        .auto_shrink([false, false])
        .show_rows(ui, 16.0, bytes.len().div_ceil(16), |ui, rows| {
            for row in rows {
                let start = row * 16;
                let chunk = &bytes[start..(start + 16).min(bytes.len())];
                let mut job = LayoutJob::default();
                let fmt = |c| TextFormat {
                    font_id: font.clone(),
                    color: c,
                    ..Default::default()
                };
                job.append(&format!("{:08x}  ", base + start as u64), 0.0, fmt(p.muted));
                let mut hex = String::with_capacity(52);
                for i in 0..16 {
                    match chunk.get(i) {
                        Some(b) => hex.push_str(&format!("{b:02x} ")),
                        None => hex.push_str("   "),
                    }
                    if i == 7 {
                        hex.push(' ');
                    }
                }
                job.append(&hex, 0.0, fmt(p.text));
                let ascii: String = chunk
                    .iter()
                    .map(|&b| {
                        if (0x20..0x7F).contains(&b) {
                            b as char
                        } else {
                            '·'
                        }
                    })
                    .collect();
                job.append(&format!(" {ascii}"), 0.0, fmt(p.accent));
                ui.label(job);
            }
        });
}
