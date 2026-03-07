//! # Kernel Logging Framework
//!
//! Structured logging for all Qernel subsystems.
//! Logs to serial console, framebuffer console, and
//! a persistent ring buffer in memory.

use alloc::collections::VecDeque;
use alloc::string::String;
use spin::Mutex;

/// Log severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Detailed trace output (disabled in production)
    Trace = 0,
    /// Debug information
    Debug = 1,
    /// Normal operational messages
    Info = 2,
    /// Warning conditions
    Warn = 3,
    /// Error conditions (recoverable)
    Error = 4,
    /// Fatal conditions (kernel panic imminent)
    Fatal = 5,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => " INFO",
            Level::Warn => " WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            Level::Trace => "\x1b[90m",   // Gray
            Level::Debug => "\x1b[36m",   // Cyan
            Level::Info  => "\x1b[32m",   // Green
            Level::Warn  => "\x1b[33m",   // Yellow
            Level::Error => "\x1b[31m",   // Red
            Level::Fatal => "\x1b[1;31m", // Bold Red
        }
    }
}

/// A log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Severity level
    pub level: Level,
    /// Source subsystem
    pub subsystem: &'static str,
    /// Log message
    pub message: String,
    /// Timestamp (tick count)
    pub timestamp: u64,
}

/// Ring buffer for in-memory log storage.
const LOG_BUFFER_SIZE: usize = 1024;

static LOG_BUFFER: Mutex<VecDeque<LogEntry>> = Mutex::new(VecDeque::new());
static MIN_LEVEL: Mutex<Level> = Mutex::new(Level::Info);

/// Set the minimum log level.
pub fn set_level(level: Level) {
    *MIN_LEVEL.lock() = level;
}

/// Log a message.
pub fn log(level: Level, subsystem: &'static str, message: String) {
    let min = *MIN_LEVEL.lock();
    if level < min {
        return;
    }

    let entry = LogEntry {
        level,
        subsystem,
        message: message.clone(),
        timestamp: crate::timer::now_ticks(),
    };

    // Print to serial
    crate::serial_println!(
        "{}{} [{}] {}\x1b[0m",
        level.color(),
        level.as_str(),
        subsystem,
        message
    );

    // Store in ring buffer
    let mut buffer = LOG_BUFFER.lock();
    if buffer.len() >= LOG_BUFFER_SIZE {
        buffer.pop_front();
    }
    buffer.push_back(entry);
}

/// Get recent log entries.
pub fn recent(count: usize) -> alloc::vec::Vec<LogEntry> {
    let buffer = LOG_BUFFER.lock();
    buffer.iter().rev().take(count).cloned().collect()
}

/// Get all log entries filtered by level.
pub fn filter_by_level(level: Level) -> alloc::vec::Vec<LogEntry> {
    let buffer = LOG_BUFFER.lock();
    buffer.iter().filter(|e| e.level >= level).cloned().collect()
}

/// Get all log entries from a specific subsystem.
pub fn filter_by_subsystem(subsystem: &str) -> alloc::vec::Vec<LogEntry> {
    let buffer = LOG_BUFFER.lock();
    buffer.iter().filter(|e| e.subsystem == subsystem).cloned().collect()
}

/// Clear all log entries.
pub fn clear() {
    LOG_BUFFER.lock().clear();
}

/// Convenience macros for logging.
#[macro_export]
macro_rules! log_trace {
    ($sub:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Trace, $sub, alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($sub:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Debug, $sub, alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($sub:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Info, $sub, alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($sub:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Warn, $sub, alloc::format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($sub:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Error, $sub, alloc::format!($($arg)*))
    };
}
