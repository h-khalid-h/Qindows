//! # Power Management — CPU P-States and Idle
//!
//! Dynamic voltage/frequency scaling and CPU idle state
//! management for energy efficiency (Section 9.28).
//!
//! Features:
//! - P-State transitions (performance/balanced/powersave)
//! - C-State idle depth control
//! - Per-core frequency tracking
//! - Thermal throttle integration
//! - Governor policies (ondemand, performance, powersave)

extern crate alloc;

use alloc::vec::Vec;

/// CPU P-State (performance state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PState {
    pub index: u8,
    pub frequency_mhz: u32,
    pub voltage_mv: u32,
}

/// CPU C-State (idle depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CState {
    C0,  // Active
    C1,  // Halt
    C1e, // Enhanced halt
    C3,  // Sleep
    C6,  // Deep sleep
    C10, // Package sleep
}

/// Governor policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Governor {
    Performance,
    OnDemand,
    PowerSave,
    Balanced,
}

/// Per-core power state.
#[derive(Debug, Clone)]
pub struct CorePower {
    pub core_id: u32,
    pub current_pstate: u8,
    pub current_cstate: CState,
    pub frequency_mhz: u32,
    pub temperature_c: u32,
    pub throttled: bool,
}

/// Power management statistics.
#[derive(Debug, Clone, Default)]
pub struct PowerStats {
    pub pstate_transitions: u64,
    pub idle_entries: u64,
    pub throttle_events: u64,
    pub total_idle_ns: u64,
}

/// The Power Manager.
pub struct PowerManager {
    pub cores: Vec<CorePower>,
    pub available_pstates: Vec<PState>,
    pub governor: Governor,
    pub thermal_limit_c: u32,
    pub stats: PowerStats,
}

impl PowerManager {
    pub fn new(num_cores: u32) -> Self {
        let mut cores = Vec::with_capacity(num_cores as usize);
        for i in 0..num_cores {
            cores.push(CorePower {
                core_id: i, current_pstate: 0, current_cstate: CState::C0,
                frequency_mhz: 3000, temperature_c: 50, throttled: false,
            });
        }

        let pstates = alloc::vec![
            PState { index: 0, frequency_mhz: 4500, voltage_mv: 1200 },
            PState { index: 1, frequency_mhz: 3600, voltage_mv: 1050 },
            PState { index: 2, frequency_mhz: 2400, voltage_mv: 900 },
            PState { index: 3, frequency_mhz: 1200, voltage_mv: 750 },
            PState { index: 4, frequency_mhz: 800,  voltage_mv: 650 },
        ];

        PowerManager {
            cores, available_pstates: pstates,
            governor: Governor::Balanced,
            thermal_limit_c: 95,
            stats: PowerStats::default(),
        }
    }

    /// Set P-State for a core.
    pub fn set_pstate(&mut self, core: u32, pstate_idx: u8) {
        if let Some(c) = self.cores.get_mut(core as usize) {
            if let Some(ps) = self.available_pstates.get(pstate_idx as usize) {
                c.current_pstate = pstate_idx;
                c.frequency_mhz = ps.frequency_mhz;
                self.stats.pstate_transitions += 1;
            }
        }
    }

    /// Enter idle state on a core.
    pub fn enter_idle(&mut self, core: u32, cstate: CState) {
        if let Some(c) = self.cores.get_mut(core as usize) {
            c.current_cstate = cstate;
            self.stats.idle_entries += 1;
        }
    }

    /// Wake a core from idle.
    pub fn wake(&mut self, core: u32) {
        if let Some(c) = self.cores.get_mut(core as usize) {
            c.current_cstate = CState::C0;
        }
    }

    /// Check thermal limits and throttle if needed.
    pub fn check_thermal(&mut self) {
        for core in &mut self.cores {
            if core.temperature_c >= self.thermal_limit_c && !core.throttled {
                core.throttled = true;
                core.frequency_mhz = core.frequency_mhz / 2;
                self.stats.throttle_events += 1;
            } else if core.temperature_c < self.thermal_limit_c - 10 && core.throttled {
                core.throttled = false;
            }
        }
    }

    /// Apply governor policy based on load.
    pub fn apply_governor(&mut self, core: u32, load_pct: u32) {
        let target_pstate = match self.governor {
            Governor::Performance => 0,
            Governor::PowerSave => self.available_pstates.len().saturating_sub(1) as u8,
            Governor::OnDemand | Governor::Balanced => {
                if load_pct > 80 { 0 }
                else if load_pct > 50 { 1 }
                else if load_pct > 20 { 2 }
                else { 3 }
            }
        };
        self.set_pstate(core, target_pstate);
    }
}
