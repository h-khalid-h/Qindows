//! # APIC Timer Silo Bridge (Phase 245)
//!
//! ## Architecture Guardian: The Gap
//! `apic_timer.rs` implements `ApicTimerManager`:
//! - `init_core(core_id, bus_freq_khz)` — initialise per-core timer
//! - `start_periodic(core_id, hz)` — start periodic IRQ at hz
//! - `arm_oneshot(core_id, delay_us)` — arm one-shot timer
//!
//! **Missing link**: `start_periodic()` had no minimum interval floor.
//! A Silo could request hz=1_000_000 (1μs period), generating an
//! interrupt storm that saturates the CPU (Law 4 DoS).
//!
//! This module provides `ApicTimerSiloBridge`:
//! Caps periodic timer to max 1000 Hz per Silo core.

extern crate alloc;

use crate::apic_timer::ApicTimerManager;

const MAX_TIMER_HZ: u32 = 1000;

#[derive(Debug, Default, Clone)]
pub struct ApicTimerSiloStats {
    pub starts_ok:     u64,
    pub hz_capped:     u64,
}

pub struct ApicTimerSiloBridge {
    pub manager: ApicTimerManager,
    pub stats:   ApicTimerSiloStats,
}

impl ApicTimerSiloBridge {
    pub fn new() -> Self {
        ApicTimerSiloBridge { manager: ApicTimerManager::new(), stats: ApicTimerSiloStats::default() }
    }

    pub fn start_periodic(&mut self, core_id: u32, hz: u32) {
        let actual = if hz > MAX_TIMER_HZ {
            self.stats.hz_capped += 1;
            crate::serial_println!(
                "[APIC TIMER] core {} requested {} Hz, capped to {} Hz", core_id, hz, MAX_TIMER_HZ
            );
            MAX_TIMER_HZ
        } else { hz };
        self.stats.starts_ok += 1;
        self.manager.start_periodic(core_id, actual);
    }

    pub fn arm_oneshot(&mut self, core_id: u32, delay_us: u64) {
        self.manager.arm_oneshot(core_id, delay_us);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ApicTimerBridge: starts={} capped={}", self.stats.starts_ok, self.stats.hz_capped
        );
    }
}
