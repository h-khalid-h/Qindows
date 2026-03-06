//! # Thermal Monitor — Zone Sensors, Trip Points, Cooling Policy
//!
//! Monitors system thermal zones and triggers cooling
//! actions to prevent hardware damage (Section 12.3).
//!
//! Features:
//! - Multiple thermal zones (CPU, GPU, SoC, battery)
//! - Configurable trip points (passive, active, critical)
//! - Cooling policies: fan control, frequency throttling, Silo migration
//! - Hysteresis to avoid oscillation
//! - Integration with Power Governor

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Thermal zone type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    Cpu,
    Gpu,
    Soc,
    Battery,
    Storage,
    Ambient,
}

/// Trip point action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TripAction {
    None = 0,
    Passive = 1,   // Throttle frequency
    Active = 2,    // Increase fan speed
    Hot = 3,       // Migrate workloads
    Critical = 4,  // Emergency shutdown
}

/// A thermal zone.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub id: u32,
    pub name: String,
    pub zone_type: ZoneType,
    pub current_temp: i32,  // millidegrees C
    pub trips: Vec<TripPoint>,
    pub hysteresis: i32,    // millidegrees
    pub last_action: TripAction,
}

/// A trip point.
#[derive(Debug, Clone)]
pub struct TripPoint {
    pub temp: i32,   // millidegrees C
    pub action: TripAction,
}

/// Thermal statistics.
#[derive(Debug, Clone, Default)]
pub struct ThermalStats {
    pub readings: u64,
    pub passive_events: u64,
    pub active_events: u64,
    pub hot_events: u64,
    pub critical_events: u64,
    pub peak_temp: i32,
}

/// The Thermal Monitor.
pub struct ThermalMonitor {
    pub zones: BTreeMap<u32, ThermalZone>,
    pub fan_speed: u8, // 0-255
    pub stats: ThermalStats,
}

impl ThermalMonitor {
    pub fn new() -> Self {
        ThermalMonitor {
            zones: BTreeMap::new(),
            fan_speed: 0,
            stats: ThermalStats::default(),
        }
    }

    /// Register a thermal zone.
    pub fn add_zone(&mut self, id: u32, name: &str, zone_type: ZoneType, hysteresis: i32) {
        self.zones.insert(id, ThermalZone {
            id, name: String::from(name), zone_type,
            current_temp: 25_000, trips: Vec::new(),
            hysteresis, last_action: TripAction::None,
        });
    }

    /// Add a trip point to a zone.
    pub fn add_trip(&mut self, zone_id: u32, temp: i32, action: TripAction) {
        if let Some(zone) = self.zones.get_mut(&zone_id) {
            zone.trips.push(TripPoint { temp, action });
            zone.trips.sort_by_key(|t| t.temp);
        }
    }

    /// Update temperature reading and return triggered actions.
    pub fn update(&mut self, zone_id: u32, temp: i32) -> Vec<TripAction> {
        self.stats.readings += 1;
        if temp > self.stats.peak_temp {
            self.stats.peak_temp = temp;
        }

        let zone = match self.zones.get_mut(&zone_id) {
            Some(z) => z,
            None => return Vec::new(),
        };

        zone.current_temp = temp;
        let mut actions = Vec::new();

        // Find highest triggered trip point
        let mut highest = TripAction::None;
        for trip in &zone.trips {
            let effective_temp = if zone.last_action >= trip.action {
                // Apply hysteresis when cooling down
                trip.temp - zone.hysteresis
            } else {
                trip.temp
            };

            if temp >= effective_temp {
                highest = trip.action;
            }
        }

        // Only emit action on transition
        if highest != zone.last_action {
            match highest {
                TripAction::Passive => self.stats.passive_events += 1,
                TripAction::Active => self.stats.active_events += 1,
                TripAction::Hot => self.stats.hot_events += 1,
                TripAction::Critical => self.stats.critical_events += 1,
                TripAction::None => {}
            }
            actions.push(highest);
            zone.last_action = highest;
        }

        // Adjust fan speed
        self.fan_speed = match highest {
            TripAction::None | TripAction::Passive => 0,
            TripAction::Active => 128,
            TripAction::Hot => 200,
            TripAction::Critical => 255,
        };

        actions
    }

    /// Get all zone temperatures.
    pub fn temperatures(&self) -> Vec<(u32, i32)> {
        self.zones.values().map(|z| (z.id, z.current_temp)).collect()
    }
}
