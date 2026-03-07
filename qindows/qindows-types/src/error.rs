//! # Error Types
//!
//! Unified error type for the Qindows ecosystem.

use core::fmt;

/// Unified Qindows error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QError {
    /// Permission denied (missing capability)
    PermissionDenied,
    /// Resource not found
    NotFound,
    /// Resource already exists
    AlreadyExists,
    /// Out of memory
    OutOfMemory,
    /// I/O error
    IoError,
    /// Invalid argument
    InvalidArgument,
    /// Operation timed out
    Timeout,
    /// Channel closed
    ChannelClosed,
    /// Buffer full (would block)
    WouldBlock,
    /// Operation not supported
    NotSupported,
    /// Silo has been killed
    SiloKilled,
    /// Quota exceeded
    QuotaExceeded,
    /// Authentication failed
    AuthFailed,
    /// Data corrupted
    Corrupted,
}

impl fmt::Display for QError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QError::PermissionDenied => write!(f, "permission denied"),
            QError::NotFound => write!(f, "not found"),
            QError::AlreadyExists => write!(f, "already exists"),
            QError::OutOfMemory => write!(f, "out of memory"),
            QError::IoError => write!(f, "I/O error"),
            QError::InvalidArgument => write!(f, "invalid argument"),
            QError::Timeout => write!(f, "timeout"),
            QError::ChannelClosed => write!(f, "channel closed"),
            QError::WouldBlock => write!(f, "would block"),
            QError::NotSupported => write!(f, "not supported"),
            QError::SiloKilled => write!(f, "silo killed"),
            QError::QuotaExceeded => write!(f, "quota exceeded"),
            QError::AuthFailed => write!(f, "authentication failed"),
            QError::Corrupted => write!(f, "data corrupted"),
        }
    }
}

/// Qindows Result type.
pub type QResult<T> = Result<T, QError>;
