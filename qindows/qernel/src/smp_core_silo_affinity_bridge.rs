//! # SMP Core Silo Affinity Bridge (Phase 270)
//!
//! ## Architecture Guardian: The Gap
//! `smp.rs` implements SMP multi-core management:
//! - Core initialization, IPI (inter-processor interrupt) vectors
//! - Per-core state management
//!
//! **Missing link**: CPU core affinity pinning for Silos had no
//! Admin:EXEC gate. Any Silo could pin itself to a specific CPU core,
//! monopolizing a core and starving all other Silos of that core's
//! scheduler time.
//!
//! This module provides `SmpCoreSiloAffinityBridge`:
//! Admin:EXEC cap required for explicit core affinity pinning.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct SmpAffinityStats {
    pub pinnings_allowed: u64,
    pub pinnings_denied:  u64,
}

pub struct SmpCoreSiloAffinityBridge {
    pub stats: SmpAffinityStats,
}

impl SmpCoreSiloAffinityBridge {
    pub fn new() -> Self {
        SmpCoreSiloAffinityBridge { stats: SmpAffinityStats::default() }
    }

    /// Authorize explicit CPU core affinity pinning — requires Admin:EXEC.
    pub fn authorize_pin(
        &mut self,
        silo_id: u64,
        core_id: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.pinnings_denied += 1;
            crate::serial_println!(
                "[SMP] Silo {} pin to core {} denied — Admin:EXEC required", silo_id, core_id
            );
            return false;
        }
        self.stats.pinnings_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SmpAffinityBridge: allowed={} denied={}",
            self.stats.pinnings_allowed, self.stats.pinnings_denied
        );
    }
}
