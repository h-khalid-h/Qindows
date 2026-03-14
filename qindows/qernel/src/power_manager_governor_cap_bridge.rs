//! # Power Manager Governor Cap Bridge (Phase 278)
//!
//! ## Architecture Guardian: The Gap
//! `power_mgmt.rs` implements `PowerManager`:
//! - `Governor` — Performance, Powersave, OnDemand, Conservative
//! - `CorePower { p_state, c_state, governor }`
//! - `PState { freq_mhz, voltage_mv }`
//!
//! **Missing link**: `Governor::Performance` mode could be set by any
//! Silo, immediately maximizing CPU frequency for all cores. This
//! bypassed thermal protection and Law 8 energy proportionality.
//!
//! This module provides `PowerManagerGovernorCapBridge`:
//! Admin:EXEC cap required to set Governor::Performance.

extern crate alloc;

use crate::power_mgmt::Governor;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct GovernorCapStats {
    pub sets_allowed: u64,
    pub sets_denied:  u64,
}

pub struct PowerManagerGovernorCapBridge {
    pub stats: GovernorCapStats,
}

impl PowerManagerGovernorCapBridge {
    pub fn new() -> Self {
        PowerManagerGovernorCapBridge { stats: GovernorCapStats::default() }
    }

    /// Authorize governor change — Performance mode requires Admin:EXEC.
    pub fn authorize_governor_set(
        &mut self,
        silo_id: u64,
        governor: &Governor,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let needs_cap = matches!(governor, Governor::Performance);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.sets_denied += 1;
            crate::serial_println!(
                "[POWER MGT] Silo {} Performance governor denied — Admin:EXEC required", silo_id
            );
            return false;
        }
        self.stats.sets_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GovernorCapBridge: allowed={} denied={}", self.stats.sets_allowed, self.stats.sets_denied
        );
    }
}
