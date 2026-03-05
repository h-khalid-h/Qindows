//! # Power Governor — Thermal-Aware Scheduling
//!
//! Dynamically adjusts CPU/GPU/NPU frequencies and core parking
//! based on thermal state, battery, and workload (Section 9).
//!
//! Policies:
//! - **Performance**: Max clocks, fans aggressive
//! - **Balanced**: Dynamic scaling based on load
//! - **Efficiency**: Prefer E-cores, park P-cores when idle
//! - **Emergency**: Thermal throttle — reduce clocks to prevent damage

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Power policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPolicy {
    Performance,
    Balanced,
    Efficiency,
    Emergency,
}

/// Core type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreType {
    Performance, // P-core
    Efficiency,  // E-core
    Gpu,
    Npu,
}

/// Core state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    Active,
    Idle,
    Parked, // Deep sleep, not schedulable
    Throttled,
}

/// A CPU/GPU/NPU core.
#[derive(Debug, Clone)]
pub struct Core {
    pub id: u32,
    pub core_type: CoreType,
    pub state: CoreState,
    /// Current frequency (MHz)
    pub freq_mhz: u32,
    /// Max frequency (MHz)
    pub max_freq: u32,
    /// Min frequency (MHz)
    pub min_freq: u32,
    /// Temperature (°C × 10 for precision)
    pub temp_c10: u32,
    /// Load percentage (0-100)
    pub load_pct: u8,
}

/// Thermal zone.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub id: u32,
    pub name: &'static str,
    pub temp_c10: u32,
    pub trip_passive: u32, // Passive cooling threshold
    pub trip_critical: u32, // Emergency shutdown threshold
}

/// Battery state.
#[derive(Debug, Clone, Copy)]
pub struct BatteryState {
    /// Charge percentage (0-100)
    pub charge_pct: u8,
    /// Is plugged in?
    pub plugged: bool,
    /// Discharge rate (mW)
    pub discharge_mw: u32,
    /// Time to empty (minutes, 0 = plugged)
    pub time_to_empty_min: u32,
}

/// Power Governor statistics.
#[derive(Debug, Clone, Default)]
pub struct GovStats {
    pub freq_changes: u64,
    pub cores_parked: u64,
    pub cores_unparked: u64,
    pub throttle_events: u64,
    pub emergency_events: u64,
    pub policy_switches: u64,
}

/// The Power Governor.
pub struct PowerGovernor {
    pub cores: BTreeMap<u32, Core>,
    pub thermals: Vec<ThermalZone>,
    pub battery: BatteryState,
    pub policy: PowerPolicy,
    pub stats: GovStats,
}

impl PowerGovernor {
    pub fn new() -> Self {
        PowerGovernor {
            cores: BTreeMap::new(),
            thermals: Vec::new(),
            battery: BatteryState { charge_pct: 100, plugged: true, discharge_mw: 0, time_to_empty_min: 0 },
            policy: PowerPolicy::Balanced,
            stats: GovStats::default(),
        }
    }

    /// Register a core.
    pub fn add_core(&mut self, id: u32, core_type: CoreType, max_freq: u32, min_freq: u32) {
        self.cores.insert(id, Core {
            id, core_type, state: CoreState::Active,
            freq_mhz: max_freq, max_freq, min_freq,
            temp_c10: 400, load_pct: 0,
        });
    }

    /// Update thermal reading.
    pub fn update_thermal(&mut self, zone_id: u32, temp_c10: u32) {
        if let Some(zone) = self.thermals.iter_mut().find(|z| z.id == zone_id) {
            zone.temp_c10 = temp_c10;
        }
    }

    /// Run the governor tick (called periodically).
    pub fn tick(&mut self) {
        // Check for thermal emergency
        let max_temp = self.thermals.iter().map(|z| z.temp_c10).max().unwrap_or(0);
        let critical = self.thermals.iter().any(|z| z.temp_c10 >= z.trip_critical);
        let passive = self.thermals.iter().any(|z| z.temp_c10 >= z.trip_passive);

        if critical && self.policy != PowerPolicy::Emergency {
            self.policy = PowerPolicy::Emergency;
            self.stats.emergency_events += 1;
            self.stats.policy_switches += 1;
        } else if passive && self.policy == PowerPolicy::Performance {
            self.policy = PowerPolicy::Balanced;
            self.stats.policy_switches += 1;
        }

        // Apply policy to cores
        let target_freq_ratio = match self.policy {
            PowerPolicy::Performance => 100u32,
            PowerPolicy::Balanced => {
                if max_temp > 800 { 70 } else { 85 }
            }
            PowerPolicy::Efficiency => 50,
            PowerPolicy::Emergency => 30,
        };

        let core_ids: Vec<u32> = self.cores.keys().copied().collect();
        for core_id in core_ids {
            if let Some(core) = self.cores.get_mut(&core_id) {
                match self.policy {
                    PowerPolicy::Efficiency => {
                        // Park P-cores when idle
                        if core.core_type == CoreType::Performance && core.load_pct < 10 {
                            if core.state != CoreState::Parked {
                                core.state = CoreState::Parked;
                                self.stats.cores_parked += 1;
                            }
                            continue;
                        }
                    }
                    PowerPolicy::Emergency => {
                        // Throttle everything (only count transition)
                        if core.state != CoreState::Throttled {
                            core.state = CoreState::Throttled;
                            self.stats.throttle_events += 1;
                        }
                    }
                    _ => {
                        if core.state == CoreState::Parked {
                            core.state = CoreState::Active;
                            self.stats.cores_unparked += 1;
                        }
                    }
                }

                let new_freq = core.min_freq + (core.max_freq - core.min_freq) * target_freq_ratio / 100;
                if core.freq_mhz != new_freq {
                    core.freq_mhz = new_freq;
                    self.stats.freq_changes += 1;
                }
            }
        }
    }

    /// Set power policy manually.
    pub fn set_policy(&mut self, policy: PowerPolicy) {
        self.policy = policy;
        self.stats.policy_switches += 1;
    }

    /// Update battery state.
    pub fn update_battery(&mut self, state: BatteryState) {
        self.battery = state;
        // Auto-switch to efficiency on low battery
        if !state.plugged && state.charge_pct < 15 && self.policy == PowerPolicy::Performance {
            self.policy = PowerPolicy::Efficiency;
            self.stats.policy_switches += 1;
        }
    }
}
