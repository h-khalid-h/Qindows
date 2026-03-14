//! # Disk Scheduler I/O Priority Bridge (Phase 289)
//!
//! ## Architecture Guardian: The Gap
//! `disk_sched.rs` implements `IoRequest`:
//! - `IoPriority` — RealTime, Interactive, Normal, Idle
//! - `SiloShare { silo_id, weight }` — per-Silo I/O share
//! - `SchedStats { total_ios, ... }` — scheduler statistics
//!
//! **Missing link**: `IoPriority::RealTime` scheduling was unrestricted.
//! Any Silo could mark its I/O requests as RealTime, preempting all
//! Normal and Idle requests and starving bulk file operations.
//!
//! This module provides `DiskSchedIoPriorityBridge`:
//! Admin:EXEC cap required to submit RealTime priority I/O.

extern crate alloc;

use crate::disk_sched::IoPriority;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct IoPriorityCapStats {
    pub requests_allowed: u64,
    pub requests_denied:  u64,
}

pub struct DiskSchedIoPriorityBridge {
    pub stats: IoPriorityCapStats,
}

impl DiskSchedIoPriorityBridge {
    pub fn new() -> Self {
        DiskSchedIoPriorityBridge { stats: IoPriorityCapStats::default() }
    }

    /// Authorize I/O priority — RealTime requires Admin:EXEC.
    pub fn authorize_priority(
        &mut self,
        silo_id: u64,
        priority: &IoPriority,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let needs_cap = matches!(priority, IoPriority::Critical | IoPriority::System);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.requests_denied += 1;
            crate::serial_println!(
                "[DISK SCHED] Silo {} Critical/System I/O priority denied — Admin:EXEC required", silo_id
            );
            return false;
        }
        self.stats.requests_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IoPriorityCapBridge: allowed={} denied={}",
            self.stats.requests_allowed, self.stats.requests_denied
        );
    }
}
