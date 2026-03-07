//! # HPET — High Precision Event Timer
//!
//! Manages the HPET hardware timer for high-resolution
//! timekeeping and calibration (Section 9.26).
//!
//! Features:
//! - Main counter (64-bit monotonic)
//! - Comparator timers (periodic/one-shot)
//! - Frequency discovery via ACPI HPET table
//! - Nanosecond-resolution timestamps
//! - Used to calibrate APIC timer and TSC

extern crate alloc;

use alloc::vec::Vec;

/// HPET timer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HpetTimerMode {
    Disabled,
    OneShot,
    Periodic,
}

/// A single HPET comparator timer.
#[derive(Debug, Clone)]
pub struct HpetTimer {
    pub index: u8,
    pub mode: HpetTimerMode,
    pub comparator_value: u64,
    pub irq: u8,
    pub irqs_fired: u64,
    pub supports_periodic: bool,
    pub supports_64bit: bool,
}

/// HPET statistics.
#[derive(Debug, Clone, Default)]
pub struct HpetStats {
    pub reads: u64,
    pub timer_irqs: u64,
    pub calibrations: u64,
}

/// The HPET Controller.
pub struct Hpet {
    pub base_addr: u64,
    pub period_fs: u64,        // femtoseconds per tick
    pub frequency_hz: u64,
    pub num_timers: u8,
    pub timers: Vec<HpetTimer>,
    pub counter_is_64bit: bool,
    pub enabled: bool,
    pub stats: HpetStats,
}

impl Hpet {
    /// Create from ACPI HPET table data.
    pub fn new(base_addr: u64, period_fs: u64, num_timers: u8) -> Self {
        let frequency_hz = if period_fs > 0 {
            1_000_000_000_000_000 / period_fs
        } else {
            0
        };

        let mut timers = Vec::with_capacity(num_timers as usize);
        for i in 0..num_timers {
            timers.push(HpetTimer {
                index: i,
                mode: HpetTimerMode::Disabled,
                comparator_value: 0,
                irq: 0,
                irqs_fired: 0,
                supports_periodic: i < 2, // Usually first 2 support periodic
                supports_64bit: true,
            });
        }

        Hpet {
            base_addr, period_fs, frequency_hz, num_timers,
            timers, counter_is_64bit: true, enabled: false,
            stats: HpetStats::default(),
        }
    }

    /// Enable the main counter.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Read main counter value.
    pub fn read_counter(&mut self) -> u64 {
        self.stats.reads += 1;
        // In production: MMIO read from base_addr + 0xF0
        0
    }

    /// Convert ticks to nanoseconds.
    pub fn ticks_to_ns(&self, ticks: u64) -> u64 {
        if self.period_fs == 0 { return 0; }
        (ticks as u128 * self.period_fs as u128 / 1_000_000) as u64
    }

    /// Convert nanoseconds to ticks.
    pub fn ns_to_ticks(&self, ns: u64) -> u64 {
        if self.period_fs == 0 { return 0; }
        (ns as u128 * 1_000_000 / self.period_fs as u128) as u64
    }

    /// Arm a one-shot timer.
    pub fn arm_oneshot(&mut self, timer: u8, delay_ns: u64, irq: u8) -> bool {
        let ticks = self.ns_to_ticks(delay_ns);
        if let Some(t) = self.timers.get_mut(timer as usize) {
            t.mode = HpetTimerMode::OneShot;
            t.comparator_value = ticks;
            t.irq = irq;
            true
        } else { false }
    }

    /// Arm a periodic timer.
    pub fn arm_periodic(&mut self, timer: u8, interval_ns: u64, irq: u8) -> bool {
        let ticks = self.ns_to_ticks(interval_ns);
        if let Some(t) = self.timers.get_mut(timer as usize) {
            if !t.supports_periodic { return false; }
            t.mode = HpetTimerMode::Periodic;
            t.comparator_value = ticks;
            t.irq = irq;
            true
        } else { false }
    }

    /// Handle a timer IRQ.
    pub fn handle_irq(&mut self, timer: u8) {
        if let Some(t) = self.timers.get_mut(timer as usize) {
            t.irqs_fired += 1;
            self.stats.timer_irqs += 1;
            if t.mode == HpetTimerMode::OneShot {
                t.mode = HpetTimerMode::Disabled;
            }
        }
    }
}
