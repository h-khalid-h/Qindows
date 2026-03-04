//! # Power Manager
//!
//! Energy-proportional computing: background processes are deep-slept,
//! CPU frequency scales with demand, and the Sentinel monitors
//! energy budgets per Silo.

use alloc::vec::Vec;
use spin::Mutex;

/// CPU power states (P-states via ACPI)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuPowerState {
    /// Maximum performance — all turbo boost active
    Performance,
    /// Balanced — frequency scaled to workload
    Balanced,
    /// Power saver — minimum frequency
    PowerSave,
    /// Deep sleep — core halted (C6 state)
    DeepSleep,
}

/// System power states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    /// S0 — fully running
    Active,
    /// S0ix — connected standby (instant wake)
    ModernStandby,
    /// S3 — suspend to RAM
    Suspend,
    /// S4 — hibernate (Prism snapshot to disk)
    Hibernate,
    /// S5 — powered off
    PowerOff,
}

/// Per-core power profile.
#[derive(Debug, Clone)]
pub struct CorePower {
    pub core_id: u32,
    pub state: CpuPowerState,
    /// Current frequency in MHz
    pub frequency_mhz: u32,
    /// Maximum frequency
    pub max_frequency_mhz: u32,
    /// Minimum frequency
    pub min_frequency_mhz: u32,
    /// Temperature in °C
    pub temperature_c: u32,
    /// Power consumption estimate (mW)
    pub power_mw: u32,
}

/// Per-Silo energy budget.
#[derive(Debug, Clone)]
pub struct EnergyBudget {
    pub silo_id: u64,
    /// CPU time consumed (microseconds)
    pub cpu_time_us: u64,
    /// Memory pages accessed
    pub memory_pages: u64,
    /// I/O operations
    pub io_ops: u64,
    /// Energy score (0-100, higher = more energy use)
    pub energy_score: u32,
    /// Is this Silo in energy violation?
    pub in_violation: bool,
}

/// The Power Manager.
pub struct PowerManager {
    /// System power state
    pub system_state: SystemState,
    /// Per-core power profiles
    pub cores: Vec<CorePower>,
    /// Per-Silo energy budgets
    pub budgets: Vec<EnergyBudget>,
    /// Energy policy
    pub policy: PowerPolicy,
    /// Battery level (0-100, None if desktop/plugged in)
    pub battery_level: Option<u8>,
}

/// Power policy modes
#[derive(Debug, Clone, Copy)]
pub enum PowerPolicy {
    /// Maximum performance, no throttling
    MaxPerformance,
    /// Smart balance (default) — scales with usage
    Adaptive,
    /// Battery saver — aggressive throttling
    BatterySaver,
    /// Silent mode — reduce fan noise (thermal throttle)
    Silent,
}

impl PowerManager {
    pub fn new(num_cores: usize) -> Self {
        let mut cores = Vec::with_capacity(num_cores);
        for i in 0..num_cores {
            cores.push(CorePower {
                core_id: i as u32,
                state: CpuPowerState::Balanced,
                frequency_mhz: 2000,
                max_frequency_mhz: 5000,
                min_frequency_mhz: 800,
                temperature_c: 40,
                power_mw: 5000,
            });
        }

        PowerManager {
            system_state: SystemState::Active,
            cores,
            budgets: Vec::new(),
            policy: PowerPolicy::Adaptive,
            battery_level: None,
        }
    }

    /// Check if a Silo is using too much energy.
    pub fn check_energy_budget(&mut self, silo_id: u64) -> bool {
        if let Some(budget) = self.budgets.iter_mut().find(|b| b.silo_id == silo_id) {
            // Calculate energy score based on usage
            budget.energy_score = ((budget.cpu_time_us / 1000)
                + budget.memory_pages
                + budget.io_ops * 10) as u32;

            // Threshold depends on policy
            let threshold = match self.policy {
                PowerPolicy::MaxPerformance => 10_000,
                PowerPolicy::Adaptive => 5_000,
                PowerPolicy::BatterySaver => 1_000,
                PowerPolicy::Silent => 2_000,
            };

            budget.in_violation = budget.energy_score > threshold;
            budget.in_violation
        } else {
            false
        }
    }

    /// Request a system state transition.
    pub fn request_state(&mut self, state: SystemState) {
        match state {
            SystemState::ModernStandby => {
                // Park all but one core, reduce frequency
                for core in &mut self.cores[1..] {
                    core.state = CpuPowerState::DeepSleep;
                }
                self.cores[0].state = CpuPowerState::PowerSave;
            }
            SystemState::Suspend => {
                // Save state to RAM, halt all cores
                for core in &mut self.cores {
                    core.state = CpuPowerState::DeepSleep;
                }
            }
            SystemState::Active => {
                // Wake all cores to balanced state
                for core in &mut self.cores {
                    core.state = CpuPowerState::Balanced;
                }
            }
            _ => {}
        }
        self.system_state = state;
    }

    /// Get total system power consumption estimate (watts).
    pub fn total_power_watts(&self) -> f32 {
        self.cores.iter().map(|c| c.power_mw as f32).sum::<f32>() / 1000.0
    }
}
