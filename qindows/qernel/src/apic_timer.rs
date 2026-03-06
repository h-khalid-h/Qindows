//! # APIC Timer — Local APIC Periodic/One-Shot Timers
//!
//! Manages the local APIC timer hardware for preemptive
//! scheduling and high-resolution kernel timers (Section 9.18).
//!
//! Features:
//! - Periodic mode (scheduler tick)
//! - One-shot mode (sleep/timeout)
//! - TSC-deadline mode
//! - Per-core timer calibration
//! - Dynamic tick rate adjustment

extern crate alloc;

use alloc::vec::Vec;

/// APIC timer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApicTimerMode {
    Periodic,
    OneShot,
    TscDeadline,
}

/// Per-core APIC timer state.
#[derive(Debug, Clone)]
pub struct CoreTimer {
    pub core_id: u32,
    pub mode: ApicTimerMode,
    pub divisor: u32,
    pub initial_count: u32,
    pub current_count: u32,
    pub ticks_per_ms: u32,
    pub irq_count: u64,
    pub armed: bool,
}

/// Timer statistics.
#[derive(Debug, Clone, Default)]
pub struct ApicTimerStats {
    pub total_irqs: u64,
    pub one_shots_fired: u64,
    pub calibrations: u64,
    pub tick_adjustments: u64,
}

/// The APIC Timer Manager.
pub struct ApicTimerManager {
    pub cores: Vec<CoreTimer>,
    pub default_hz: u32,
    pub stats: ApicTimerStats,
}

impl ApicTimerManager {
    pub fn new() -> Self {
        ApicTimerManager {
            cores: Vec::new(),
            default_hz: 1000, // 1kHz default tick rate
            stats: ApicTimerStats::default(),
        }
    }

    /// Initialize timer for a core.
    pub fn init_core(&mut self, core_id: u32, bus_freq_khz: u32) {
        let divisor = 16u32;
        let ticks_per_ms = bus_freq_khz / divisor;
        let initial_count = ticks_per_ms; // 1ms tick

        self.cores.push(CoreTimer {
            core_id, mode: ApicTimerMode::Periodic, divisor,
            initial_count, current_count: initial_count,
            ticks_per_ms, irq_count: 0, armed: false,
        });
        self.stats.calibrations += 1;
    }

    /// Start periodic timer on a core.
    pub fn start_periodic(&mut self, core_id: u32, hz: u32) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.core_id == core_id) {
            core.mode = ApicTimerMode::Periodic;
            if hz > 0 {
                core.initial_count = core.ticks_per_ms * 1000 / hz;
            }
            core.current_count = core.initial_count;
            core.armed = true;
        }
    }

    /// Arm one-shot timer.
    pub fn arm_oneshot(&mut self, core_id: u32, delay_us: u64) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.core_id == core_id) {
            core.mode = ApicTimerMode::OneShot;
            core.initial_count = (core.ticks_per_ms as u64 * delay_us / 1000) as u32;
            core.current_count = core.initial_count;
            core.armed = true;
        }
    }

    /// Handle timer IRQ on a core.
    pub fn handle_irq(&mut self, core_id: u32) -> bool {
        if let Some(core) = self.cores.iter_mut().find(|c| c.core_id == core_id) {
            if !core.armed { return false; }
            core.irq_count += 1;
            self.stats.total_irqs += 1;

            match core.mode {
                ApicTimerMode::Periodic => {
                    core.current_count = core.initial_count; // Auto-reload
                    true
                }
                ApicTimerMode::OneShot => {
                    core.armed = false;
                    self.stats.one_shots_fired += 1;
                    true
                }
                ApicTimerMode::TscDeadline => {
                    core.armed = false;
                    true
                }
            }
        } else { false }
    }

    /// Stop timer on a core.
    pub fn stop(&mut self, core_id: u32) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.core_id == core_id) {
            core.armed = false;
            core.current_count = 0;
        }
    }
}
