//! # Thermal Zone Policy Bridge (Phase 190)
//!
//! ## Architecture Guardian: The Gap
//! `thermal.rs` implements `ThermalMonitor`:
//! - `ThermalMonitor::add_zone(id, name, zone_type: ZoneType, hysteresis: i32)` — add zone
//! - `ThermalMonitor::add_trip(zone_id, temp, action: TripAction)` — add trip point
//! - `ThermalMonitor::update(zone_id, temp: i32)` → Vec<TripAction>
//! - `TripAction` variants: None, Passive, Active, Hot, Critical
//!
//! **Missing link**: Thermal TripActions were detected but silently dropped.
//! This module enforces responses: logs Hot/Critical events.

extern crate alloc;
use alloc::vec::Vec;

use crate::thermal::{ThermalMonitor, TripAction, ZoneType};

#[derive(Debug, Default, Clone)]
pub struct ThermalPolicyStats {
    pub trip_events:    u64,
    pub hot_events:     u64,
    pub critical_events: u64,
}

pub struct ThermalZonePolicyBridge {
    pub monitor: ThermalMonitor,
    pub stats:   ThermalPolicyStats,
}

impl ThermalZonePolicyBridge {
    pub fn new() -> Self {
        ThermalZonePolicyBridge { monitor: ThermalMonitor::new(), stats: ThermalPolicyStats::default() }
    }

    /// Initialize standard CPU + GPU thermal zones with trip points.
    pub fn init_default_zones(&mut self) {
        self.monitor.add_zone(0, "CPU", ZoneType::Cpu, 5);
        self.monitor.add_zone(1, "GPU", ZoneType::Gpu, 5);
        self.monitor.add_trip(0, 80_000, TripAction::Passive);
        self.monitor.add_trip(0, 90_000, TripAction::Hot);
        self.monitor.add_trip(0, 100_000, TripAction::Critical);
        self.monitor.add_trip(1, 85_000, TripAction::Hot);
    }

    /// Update a thermal zone and enforce any triggered TripActions.
    pub fn update_and_enforce(&mut self, zone_id: u32, temp_millideg: i32) -> Vec<TripAction> {
        let actions = self.monitor.update(zone_id, temp_millideg);
        for action in &actions {
            self.stats.trip_events += 1;
            match action {
                TripAction::Hot => {
                    self.stats.hot_events += 1;
                    crate::serial_println!(
                        "[THERMAL] Zone {} Hot trip @ {}m°C — throttle Silos", zone_id, temp_millideg
                    );
                }
                TripAction::Critical => {
                    self.stats.critical_events += 1;
                    crate::serial_println!(
                        "[THERMAL] Zone {} CRITICAL @ {}m°C — emergency!", zone_id, temp_millideg
                    );
                }
                _ => {}
            }
        }
        actions
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ThermalBridge: trips={} hot={} critical={}",
            self.stats.trip_events, self.stats.hot_events, self.stats.critical_events
        );
    }
}
