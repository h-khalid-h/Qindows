//! # Coredump Cap Bridge (Phase 228)
//!
//! ## Architecture Guardian: The Gap
//! `coredump.rs` implements core dump structures:
//! - `DumpReason` — Panic, CapViolation, MemFault, SentinelKill, ...
//! - `MemoryRegion::is_readable() / is_writable() / is_executable()`
//! - `DumpType` — Full, Partial, ThreadOnly
//!
//! **Missing link**: Core dump read access was in kdump_admin_cap_bridge
//! but the coredump memory region analysis was unguarded. Any code
//! could examine MemoryRegion permissions from another Silo's dump.
//!
//! This module provides `CoredumpCapBridge`:
//! Admin:EXEC required to access MemoryRegion data from a coredump.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};
use crate::coredump::DumpReason;

#[derive(Debug, Default, Clone)]
pub struct CoredumpCapStats {
    pub analyses_allowed: u64,
    pub analyses_denied:  u64,
}

pub struct CoredumpCapBridge {
    pub stats: CoredumpCapStats,
}

impl CoredumpCapBridge {
    pub fn new() -> Self {
        CoredumpCapBridge { stats: CoredumpCapStats::default() }
    }

    /// Authorize analyzing another Silo's coredump memory regions.
    pub fn authorize_analysis(
        &mut self,
        analyst_silo: u64,
        victim_silo: u64,
        reason: &DumpReason,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if analyst_silo == victim_silo {
            self.stats.analyses_allowed += 1;
            return true;
        }
        if !forge.check(analyst_silo, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.analyses_denied += 1;
            crate::serial_println!(
                "[COREDUMP] Silo {} analysis of Silo {} denied — no Admin:EXEC cap",
                analyst_silo, victim_silo
            );
            return false;
        }
        self.stats.analyses_allowed += 1;
        crate::serial_println!(
            "[COREDUMP] Analysis of Silo {} authorized by Silo {} (reason: {:?})",
            victim_silo, analyst_silo, reason
        );
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CoredumpBridge: allowed={} denied={}", self.stats.analyses_allowed, self.stats.analyses_denied
        );
    }
}
