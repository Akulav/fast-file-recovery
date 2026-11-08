//! Fast File Recovery engine.
//!
//! This crate has no GUI, network or install code. The GUI talks to it only through
//! [`start_scan`], [`start_recovery`], [`start_imaging`] and the read-only device API.
//!
//! Safety rule: nothing in this crate ever writes to a source device. [`device::SourceHandle`]
//! exposes reads only, and every handle to a source is opened read-only.

pub mod carve;
pub mod device;
pub mod error;
pub mod fs;
pub mod image;
pub mod io;
pub mod kinds;
pub mod recover;
pub mod scan;
pub mod signatures;
pub mod util;

pub use error::{CoreError, Result};
pub use image::{start_imaging, ImageEvent, ImageOptions};
pub use recover::{start_recovery, RecoverEvent, RecoverOptions};
pub use scan::{start_scan, ScanEvent, ScanHandle, ScanMode, ScanOptions, Session, SourceSpec};

/// Shared handle used to cancel a recovery or imaging job.
#[derive(Clone, Default)]
pub struct JobHandle {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}
