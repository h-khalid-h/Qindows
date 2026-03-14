//! # Power Governor Energy Bridge (Phase 157)
//!
//! ## Architecture Guardian: The Gap
//! `power_gov.rs` implements `PowerGovernor`:
//! - `add_core(id, core_type, max_freq, min_freq)` — registers a CPU core
//! - `update_thermal(zone_id, temp_c10)` — feeds thermal readings
//! - `tick()` — adjusts P-state, returns nothing
//!
//! **Missing link**: `PowerGovernor::tick()` was never called from the APIC
//! timer loop. Thermal readings from `thermal.rs` were never fed in.
//!
//! This module provides `PowerGovEnergyBridge`:
//! 1. `init_topology()` — registers all CPU cores at boot
//! 2. `on_thermal_reading()` — feeds temp from thermal.rs
//! 3. `on_apic_tick()` — drives governor tick every N ticks

extern crate alloc;

use crate::power_gov::{PowerGovernor, CoreType};

#[derive(Debug, Default, Clone)]
pub struct PowerGovBridgeStats {
    pub thermal_updates: u64,
    pub gov_ticks:       u64,
}

pub struct PowerGovEnergyBridge {
    pub governor:      PowerGovernor,
    pub stats:         PowerGovBridgeStats,
    tick_interval:     u64,
    last_gov_tick:     u64,
}

impl PowerGovEnergyBridge {
    pub fn new() -> Self {
        let mut governor = PowerGovernor::new();
        // Register cores at init: core 0 = efficiency, cores 1-3 = performance
        governor.add_core(0, CoreType::Efficiency, 2_400, 800);
        governor.add_core(1, CoreType::Performance, 5_200, 1_600);
        governor.add_core(2, CoreType::Performance, 5_200, 1_600);
        governor.add_core(3, CoreType::Performance, 5_200, 1_600);

        PowerGovEnergyBridge {
            governor, stats: PowerGovBridgeStats::default(),
            tick_interval: 10, last_gov_tick: 0,
        }
    }

    /// Feed a temperature reading from the thermal subsystem.
    /// temp_c10 = temperature × 10 (e.g. 650 = 65.0°C)
    pub fn on_thermal_reading(&mut self, zone_id: u32, temp_c10: u32) {
        self.stats.thermal_updates += 1;
        self.governor.update_thermal(zone_id, temp_c10);
    }

    /// Called on each APIC timer tick; drives governor at configured interval.
    pub fn on_apic_tick(&mut self, tick: u64) {
        if tick - self.last_gov_tick >= self.tick_interval {
            self.last_gov_tick = tick;
            self.stats.gov_ticks += 1;
            self.governor.tick();
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PowerGovBridge: thermals={} ticks={}",
            self.stats.thermal_updates, self.stats.gov_ticks
        );
    }
}
