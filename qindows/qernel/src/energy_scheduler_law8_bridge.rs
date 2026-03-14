//! # Energy Scheduler Law 8 Bridge (Phase 251)
//!
//! ## Architecture Guardian: The Gap
//! `q_energy_scheduler.rs` implements `EnergyScheduler`:
//! - `register_silo(silo_id)` — add Silo to energy tracking
//! - `background_silo(silo_id)` — demote to P3 background P-state
//! - `elevate(silo_id, target: PStateTarget)` — change P-state
//! - `PStateTarget` — C3/C1/P3/P2/P1/P0/P0Boost
//!
//! **Missing link**: EnergyScheduler tracked energy but never demoted
//! overbudget Silos. P0Boost was indefinitely available without Law 8
//! enforcement even when Silo exceeded its energy budget.
//!
//! This module provides `EnergySchedulerLaw8Bridge`:
//! Calls `background_silo()` on overbudget Silos + Law 8 audit.

extern crate alloc;

use crate::q_energy_scheduler::{EnergyScheduler, PStateTarget};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct EnergyLaw8Stats {
    pub throttled: u64,
    pub compliant: u64,
}

pub struct EnergySchedulerLaw8Bridge {
    pub stats: EnergyLaw8Stats,
}

impl EnergySchedulerLaw8Bridge {
    pub fn new() -> Self {
        EnergySchedulerLaw8Bridge { stats: EnergyLaw8Stats::default() }
    }

    /// Enforce Law 8: if Silo is overbudget on energy, demote to background P-state.
    pub fn enforce(
        &mut self,
        sched: &mut EnergyScheduler,
        silo_id: u64,
        over_budget: bool,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> PStateTarget {
        if over_budget {
            self.stats.throttled += 1;
            audit.log_law_violation(8u8, silo_id, tick); // Law 8: energy proportionality
            sched.background_silo(silo_id); // Demote to P3 (~1.2 GHz)
            crate::serial_println!(
                "[ENERGY LAW 8] Silo {} over budget — demoted to P3 background, Law 8 audit", silo_id
            );
            PStateTarget::P3
        } else {
            self.stats.compliant += 1;
            PStateTarget::P1 // Standard interactive
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EnergyLaw8Bridge: throttled={} compliant={}", self.stats.throttled, self.stats.compliant
        );
    }
}
