//! # Silo Types
//!
//! Shared Silo (process container) types used across the Qindows stack.
//! A Silo is the fundamental isolation unit — analogous to a process
//! but with stronger capability-based security.

use alloc::string::String;
use alloc::vec::Vec;

/// Unique identifier for a Silo.
pub type SiloId = u64;

/// Silo lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloState {
    /// Being created (loading manifest, allocating memory)
    Creating,
    /// Ready to run but not yet scheduled
    Ready,
    /// Currently executing on a CPU core
    Running,
    /// Waiting for I/O, IPC, or a timer
    Blocked,
    /// Temporarily suspended by the user or system
    Suspended,
    /// Terminated normally
    Exited(i32),
    /// Killed due to a fault or policy violation
    Killed,
}

/// Silo priority class (maps to scheduler weight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SiloPriority {
    /// Background tasks (indexing, backups)
    Background = 0,
    /// Normal user applications
    Normal = 1,
    /// Foreground app with focus
    Foreground = 2,
    /// System services (Prism, Nexus, Sentinel)
    System = 3,
    /// Real-time (audio, input pipeline)
    Realtime = 4,
}

/// Silo resource limits.
#[derive(Debug, Clone, Copy)]
pub struct SiloLimits {
    /// Maximum memory in bytes
    pub max_memory: u64,
    /// Maximum CPU time per scheduling period (microseconds)
    pub cpu_time_us: u64,
    /// Maximum open file handles
    pub max_handles: u32,
    /// Maximum IPC channels
    pub max_channels: u32,
    /// Maximum network bandwidth (bytes/sec, 0 = unlimited)
    pub net_bandwidth: u64,
}

impl Default for SiloLimits {
    fn default() -> Self {
        SiloLimits {
            max_memory: 256 * 1024 * 1024, // 256 MiB
            cpu_time_us: 100_000,           // 100ms per period
            max_handles: 1024,
            max_channels: 64,
            net_bandwidth: 0,
        }
    }
}

/// Silo descriptor — enough info to identify and manage a Silo.
#[derive(Debug, Clone)]
pub struct SiloDescriptor {
    pub id: SiloId,
    pub name: String,
    pub state: SiloState,
    pub priority: SiloPriority,
    pub limits: SiloLimits,
    /// Capabilities granted to this Silo
    pub capabilities: Vec<super::capability::Capability>,
}
