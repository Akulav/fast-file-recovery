use serde::{Deserialize, Serialize};

/// Errors from the engine. Messages are user-facing: short, plain, and say what to do next.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Access denied. Restart Fast File Recovery with administrator rights.")]
    AccessDenied,
    #[error("Could not lock {}. Close programs that use the disk, then try again.", .0.join(", "))]
    LockFailed(Vec<String>),
    #[error("{0}")]
    Invalid(String),
    #[error("The destination is on the disk you are recovering from. Pick a folder on a different disk.")]
    SameDisk,
    #[error("Cancelled")]
    Cancelled,
    #[error("Saved scan could not be read: {0}")]
    Session(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn from_io(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            CoreError::AccessDenied
        } else {
            CoreError::Io(e)
        }
    }
}

/// Clonable error carried by scan events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanError {
    /// Locking failed for these volumes. The GUI offers retry / continue unlocked / cancel.
    LockFailed(Vec<String>),
    AccessDenied,
    Other(String),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::LockFailed(v) => write!(
                f,
                "Could not lock {}. Close programs that use the disk, then try again.",
                v.join(", ")
            ),
            ScanError::AccessDenied => {
                write!(f, "Access denied. Restart with administrator rights.")
            }
            ScanError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl From<CoreError> for ScanError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::LockFailed(v) => ScanError::LockFailed(v),
            CoreError::AccessDenied => ScanError::AccessDenied,
            other => ScanError::Other(other.to_string()),
        }
    }
}
