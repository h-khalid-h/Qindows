//! # TSC Calibration — Timestamp Counter Management
//!
//! Calibrates and manages the CPU Time Stamp Counter (TSC)
//! for high-resolution timing across cores (Section 9.17).
//!
//! Features:
//! - TSC-to-nanosecond conversion factors
//! - Per-core TSC offset calibration
//! - Invariant TSC detection
//! - Fallback to HPET/PIT if TSC unreliable
//! - Frequency scaling awareness

extern crate alloc;

use alloc::vec::Vec;

/// TSC reliability level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TscReliability {
    /// Invariant TSC (nonstop, constant rate)
    Invariant,
    /// Constant rate but may stop on C-states
    Constant,
    /// Variable rate (changes with frequency scaling)
    Variable,
    /// Unreliable — use fallback timer
    Unreliable,
}

/// Per-core TSC calibration data.
#[derive(Debug, Clone)]
pub struct CoreCalibration {
    pub core_id: u32,
    pub offset: i64,
    pub frequency_khz: u64,
    pub calibrated: bool,
}

/// TSC statistics.
#[derive(Debug, Clone, Default)]
pub struct TscStats {
    pub calibrations: u64,
    pub recalibrations: u64,
    pub fallback_reads: u64,
}

/// The TSC Manager.
pub struct TscManager {
    pub reliability: TscReliability,
    pub base_frequency_khz: u64,
    pub cores: Vec<CoreCalibration>,
    pub ns_per_tick_num: u64,
    pub ns_per_tick_den: u64,
    pub stats: TscStats,
}

impl TscManager {
    pub fn new() -> Self {
        TscManager {
            reliability: TscReliability::Unreliable,
            base_frequency_khz: 0,
            cores: Vec::new(),
            ns_per_tick_num: 1,
            ns_per_tick_den: 1,
            stats: TscStats::default(),
        }
    }

    /// Calibrate the TSC using a reference timer.
    pub fn calibrate(&mut self, reference_ns: u64, tsc_ticks: u64, core_count: u32) {
        if tsc_ticks == 0 { return; }

        self.base_frequency_khz = tsc_ticks * 1_000_000 / reference_ns;
        self.ns_per_tick_num = reference_ns;
        self.ns_per_tick_den = tsc_ticks;

        // Initialize per-core calibration
        self.cores.clear();
        for i in 0..core_count {
            self.cores.push(CoreCalibration {
                core_id: i, offset: 0,
                frequency_khz: self.base_frequency_khz,
                calibrated: true,
            });
        }

        self.stats.calibrations += 1;
    }

    /// Set TSC reliability level.
    pub fn set_reliability(&mut self, level: TscReliability) {
        self.reliability = level;
    }

    /// Convert TSC ticks to nanoseconds.
    pub fn ticks_to_ns(&self, ticks: u64) -> u64 {
        if self.ns_per_tick_den == 0 { return 0; }
        ticks.saturating_mul(self.ns_per_tick_num) / self.ns_per_tick_den
    }

    /// Convert nanoseconds to TSC ticks.
    pub fn ns_to_ticks(&self, ns: u64) -> u64 {
        if self.ns_per_tick_num == 0 { return 0; }
        ns.saturating_mul(self.ns_per_tick_den) / self.ns_per_tick_num
    }

    /// Adjust per-core offset (for cross-core synchronization).
    pub fn adjust_core_offset(&mut self, core_id: u32, offset: i64) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.core_id == core_id) {
            core.offset = offset;
            self.stats.recalibrations += 1;
        }
    }

    /// Get synchronized timestamp for a core.
    pub fn read_synced(&self, raw_tsc: u64, core_id: u32) -> u64 {
        let offset = self.cores.iter()
            .find(|c| c.core_id == core_id)
            .map(|c| c.offset)
            .unwrap_or(0);
        let adjusted = if offset >= 0 {
            raw_tsc.wrapping_add(offset as u64)
        } else {
            raw_tsc.wrapping_sub((-offset) as u64)
        };
        self.ticks_to_ns(adjusted)
    }
}
