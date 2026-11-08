//! `settings.toml`: loaded once at start, saved on a worker thread.

use ffr_core::recover::{Layout, PartialPolicy};
use ffr_core::ScanMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePref {
    Dark,
    Light,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: ThemePref,
    pub chunk_mib: u32,
    /// 0 = automatic.
    pub queue_depth: u32,
    pub retries: u32,
    pub cluster_aligned_only: bool,
    pub default_mode: ScanMode,
    pub enabled_formats: BTreeSet<String>,
    pub layout: Layout,
    pub partial_policy: PartialPolicy,
    pub write_timestamps: bool,
    pub auto_update: bool,
    pub window: Option<WindowGeom>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemePref::Dark,
            chunk_mib: 16,
            queue_depth: 0,
            retries: 2,
            cluster_aligned_only: true,
            default_mode: ScanMode::Full,
            enabled_formats: ffr_core::signatures::default_ids(),
            layout: Layout::ByType,
            partial_policy: PartialPolicy::Suffix,
            write_timestamps: true,
            auto_update: true,
            window: None,
        }
    }
}

impl Settings {
    /// Read settings (the only I/O the UI thread does). Falls back to defaults.
    pub fn load() -> Settings {
        let path = crate::paths::settings_file();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Settings::default();
        };
        match toml::from_str::<Settings>(&text) {
            Ok(s) => s.migrate(),
            Err(e) => {
                tracing::warn!("settings.toml is invalid ({e}); using defaults");
                Settings::default()
            }
        }
    }

    /// Bring older settings files forward.
    fn migrate(mut self) -> Settings {
        if self.version < SETTINGS_VERSION {
            self.version = SETTINGS_VERSION;
        }
        let known: BTreeSet<String> = ffr_core::signatures::all()
            .iter()
            .map(|s| s.id.to_string())
            .collect();
        self.enabled_formats.retain(|id| known.contains(id));
        self.chunk_mib = self.chunk_mib.clamp(4, 64);
        self.queue_depth = self.queue_depth.min(16);
        self.retries = self.retries.min(10);
        self
    }

    /// Save on a background thread.
    pub fn save_async(&self) {
        let s = self.clone();
        std::thread::spawn(move || s.save_now());
    }

    pub fn save_now(&self) {
        let path = crate::paths::settings_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string_pretty(self) {
            Ok(t) => {
                let tmp = path.with_extension("toml.tmp");
                if std::fs::write(&tmp, t).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
                crate::elevate::give_to_original_user(&path);
            }
            Err(e) => tracing::warn!("could not serialise settings: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_migrate() {
        let s = Settings::default();
        let t = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&t).unwrap();
        assert_eq!(back, s);
        let partial: Settings = toml::from_str("version = 0\nchunk_mib = 999\n").unwrap();
        let m = partial.migrate();
        assert_eq!(m.chunk_mib, 64);
        assert_eq!(m.version, SETTINGS_VERSION);
    }
}
