//! # CPU Frequency Silo Bridge (Phase 198)
//!
//! ## Architecture Guardian: The Gap
//! `cpu_freq.rs` implements `CpuFreqScaler`:
//! - `set_governor(gov: Governor)` — set frequency scaling policy
//! - `set_frequency(core_id, target_khz)` — set exact frequency
//! - `evaluate(core_id, load_percent)` — auto-scale based on load
//! - `Governor` variants: Performance, PowerSave, OnDemand, Conservative, Schedutil
//!
//! **Missing link**: `set_governor()` and `set_frequency()` were accessible
//! without Admin:EXEC cap. A compromised Silo could lock all cores to
//! max frequency, causing thermal overrun (Law 8 violation).
//!
//! This module provides `CpuFreqSiloCapBridge`:
//! Admin:EXEC required for governor/frequency changes.

extern crate alloc;

use crate::cpu_freq::{CpuFreqScaler, Governor};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct CpuFreqCapStats {
    pub gov_sets_allowed: u64,
    pub gov_sets_denied:  u64,
    pub freq_sets_allowed: u64,
    pub freq_sets_denied:  u64,
}

pub struct CpuFreqSiloCapBridge {
    pub scaler: CpuFreqScaler,
    pub stats:  CpuFreqCapStats,
}

impl CpuFreqSiloCapBridge {
    pub fn new(num_cores: u32) -> Self {
        // Initialize with common x86 frequency table
        let freq_table = alloc::vec![1_000_000, 1_500_000, 2_000_000, 2_500_000, 3_000_000, 3_500_000, 4_000_000];
        CpuFreqSiloCapBridge { scaler: CpuFreqScaler::new(num_cores, freq_table), stats: CpuFreqCapStats::default() }
    }

    /// Set frequency governor — requires Admin:EXEC cap.
    pub fn set_governor(
        &mut self,
        silo_id: u64,
        gov: Governor,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.gov_sets_denied += 1;
            crate::serial_println!("[CPU FREQ] Silo {} governor change denied", silo_id);
            return false;
        }
        self.stats.gov_sets_allowed += 1;
        self.scaler.set_governor(gov);
        true
    }

    /// Set explicit core frequency — requires Admin:EXEC cap.
    pub fn set_frequency(
        &mut self,
        silo_id: u64,
        core_id: u32,
        target_khz: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.freq_sets_denied += 1;
            return false;
        }
        self.stats.freq_sets_allowed += 1;
        self.scaler.set_frequency(core_id, target_khz);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CpuFreqBridge: gov={}/{} freq={}/{}",
            self.stats.gov_sets_allowed, self.stats.gov_sets_denied,
            self.stats.freq_sets_allowed, self.stats.freq_sets_denied
        );
    }
}
