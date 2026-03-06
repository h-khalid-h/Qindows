//! # CPU Frequency Scaling — Per-Core DVFS Governor
//!
//! Controls per-core voltage and frequency for power/performance
//! optimization (Section 9.8).
//!
//! Features:
//! - Per-core frequency control
//! - Governor policies (performance, powersave, ondemand, schedutil)
//! - Frequency transition latency tracking
//! - Per-Silo frequency hints
//! - Boost mode control (turbo)

extern crate alloc;

use alloc::collections::BTreeMap;

/// Frequency governor policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Governor {
    Performance,
    Powersave,
    Ondemand,
    Schedutil,
}

/// Per-core frequency state.
#[derive(Debug, Clone)]
pub struct CoreFreqState {
    pub core_id: u32,
    pub current_khz: u32,
    pub min_khz: u32,
    pub max_khz: u32,
    pub governor: Governor,
    pub boost_enabled: bool,
    pub transitions: u64,
    pub time_in_state_ms: BTreeMap<u32, u64>, // freq_khz → time
}

/// CPU frequency statistics.
#[derive(Debug, Clone, Default)]
pub struct CpuFreqStats {
    pub total_transitions: u64,
    pub boost_activations: u64,
    pub gov_changes: u64,
}

/// The CPU Frequency Scaler.
pub struct CpuFreqScaler {
    pub cores: BTreeMap<u32, CoreFreqState>,
    pub global_governor: Governor,
    pub global_boost: bool,
    /// Available frequency steps (kHz)
    pub freq_table: alloc::vec::Vec<u32>,
    pub stats: CpuFreqStats,
}

impl CpuFreqScaler {
    pub fn new(num_cores: u32, freq_table: alloc::vec::Vec<u32>) -> Self {
        let min_khz = freq_table.first().copied().unwrap_or(800_000);
        let max_khz = freq_table.last().copied().unwrap_or(4_000_000);

        let mut cores = BTreeMap::new();
        for i in 0..num_cores {
            cores.insert(i, CoreFreqState {
                core_id: i, current_khz: max_khz,
                min_khz, max_khz,
                governor: Governor::Performance,
                boost_enabled: true, transitions: 0,
                time_in_state_ms: BTreeMap::new(),
            });
        }

        CpuFreqScaler {
            cores, global_governor: Governor::Performance,
            global_boost: true, freq_table,
            stats: CpuFreqStats::default(),
        }
    }

    /// Set governor for all cores.
    pub fn set_governor(&mut self, gov: Governor) {
        self.global_governor = gov;
        for core in self.cores.values_mut() {
            core.governor = gov;
        }
        self.stats.gov_changes += 1;
    }

    /// Set frequency for a specific core.
    pub fn set_frequency(&mut self, core_id: u32, target_khz: u32) {
        if let Some(core) = self.cores.get_mut(&core_id) {
            let clamped = target_khz.max(core.min_khz).min(core.max_khz);
            // Find nearest available step
            let freq = self.freq_table.iter()
                .min_by_key(|&&f| (f as i64 - clamped as i64).unsigned_abs())
                .copied()
                .unwrap_or(clamped);

            if freq != core.current_khz {
                core.current_khz = freq;
                core.transitions += 1;
                self.stats.total_transitions += 1;
            }
        }
    }

    /// Evaluate ondemand governor for a core based on load.
    pub fn evaluate(&mut self, core_id: u32, load_percent: u8) {
        let gov = match self.cores.get(&core_id) {
            Some(c) => c.governor,
            None => return,
        };

        match gov {
            Governor::Performance => {
                if let Some(c) = self.cores.get(&core_id) {
                    let max = c.max_khz;
                    self.set_frequency(core_id, max);
                }
            }
            Governor::Powersave => {
                if let Some(c) = self.cores.get(&core_id) {
                    let min = c.min_khz;
                    self.set_frequency(core_id, min);
                }
            }
            Governor::Ondemand | Governor::Schedutil => {
                if let Some(c) = self.cores.get(&core_id) {
                    let range = c.max_khz - c.min_khz;
                    let target = c.min_khz + (range as u64 * load_percent as u64 / 100) as u32;
                    self.set_frequency(core_id, target);
                }
            }
        }
    }

    /// Toggle boost (turbo) mode.
    pub fn set_boost(&mut self, enabled: bool) {
        self.global_boost = enabled;
        for core in self.cores.values_mut() {
            core.boost_enabled = enabled;
        }
        if enabled { self.stats.boost_activations += 1; }
    }
}
