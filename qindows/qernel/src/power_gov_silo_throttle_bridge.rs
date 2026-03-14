//! # Power Governor Silo Throttle Bridge (Phase 185)
//!
//! ## Architecture Guardian: The Gap
//! `power_gov.rs` implements `PowerGovernor`:
//! - Manages per-core `PowerPolicy` / `CoreState` / `ThermalZone`
//! - Per-Silo energy budgets via `GovStats`
//!
//! **Missing link**: `PowerGovernor` tracked power policies globally but
//! never throttled individual Silos. A Silo running at full CPU still
//! consumed power even when the system was in thermal overrun state —
//! violating Law 8 (no infinite loops/resource monopoly) in practice.
//!
//! This module provides `PowerGovSiloThrottleBridge`:
//! 1. `on_thermal_alert()` — throttle Silos on thermal overrun
//! 2. `on_silo_tick()` — subtract energy budget on each tick

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::power_gov::PowerGovernor;

#[derive(Debug, Default, Clone)]
pub struct PowerGovBridgeStats {
    pub thermal_throttles: u64,
    pub silos_throttled:   u64,
    pub budget_exhausted:  u64,
}

struct SiloPowerState {
    energy_budget: u64,
    throttled:     bool,
}

pub struct PowerGovSiloThrottleBridge {
    pub governor:  PowerGovernor,
    silo_power:    BTreeMap<u64, SiloPowerState>,
    pub stats:     PowerGovBridgeStats,
}

impl PowerGovSiloThrottleBridge {
    pub fn new() -> Self {
        PowerGovSiloThrottleBridge {
            governor: PowerGovernor::new(),
            silo_power: BTreeMap::new(),
            stats: PowerGovBridgeStats::default(),
        }
    }

    /// Set energy budget for a Silo (in energy ticks).
    pub fn set_silo_energy_budget(&mut self, silo_id: u64, budget: u64) {
        self.silo_power.insert(silo_id, SiloPowerState {
            energy_budget: budget, throttled: false,
        });
    }

    /// Called on each scheduler tick for a Silo. Returns false = throttle this Silo.
    pub fn on_silo_tick(&mut self, silo_id: u64) -> bool {
        let state = self.silo_power.entry(silo_id).or_insert(SiloPowerState {
            energy_budget: 10_000, throttled: false,
        });

        if state.throttled {
            return false;
        }
        if state.energy_budget == 0 {
            self.stats.budget_exhausted += 1;
            crate::serial_println!("[PWR GOV] Silo {} energy budget exhausted — throttled (Law 8)", silo_id);
            state.throttled = true;
            return false;
        }
        state.energy_budget -= 1;
        true
    }

    /// Throttle all Silos on thermal overrun. Governor decides policy.
    pub fn on_thermal_alert(&mut self) {
        self.stats.thermal_throttles += 1;
        for (_, state) in self.silo_power.iter_mut() {
            state.throttled = true;
            self.stats.silos_throttled += 1;
        }
        crate::serial_println!("[PWR GOV] Thermal alert: all Silos throttled");
    }

    /// Restore Silo scheduling after thermal event clears.
    pub fn on_thermal_clear(&mut self) {
        for state in self.silo_power.values_mut() {
            state.throttled = false;
        }
        crate::serial_println!("[PWR GOV] Thermal clear: Silos restored");
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PowerGovBridge: throttles={} silos={} budget_exhausted={}",
            self.stats.thermal_throttles, self.stats.silos_throttled, self.stats.budget_exhausted
        );
    }
}
