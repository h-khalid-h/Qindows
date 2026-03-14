//! # Timer Wheel Silo Bridge (Phase 166)
//!
//! ## Architecture Guardian: The Gap
//! `timer_wheel.rs` implements `TimerWheel`:
//! - `new(tick_ns)` — timer resolution in nanoseconds
//! - `schedule(delay_ns, silo_id, tag)` → TimerId
//! - `schedule_repeating(interval_ns, silo_id, tag)` → TimerId
//! - `cancel(id)` → bool
//!
//! **Missing link**: `TimerWheel` was never integrated with Silo lifecycle.
//! Timers for vaporized Silos kept firing, and no cleanup freed expired
//! timer slots.
//!
//! This module provides `TimerWheelSiloBridge`:
//! 1. `schedule_for_silo()` — register per-Silo timer with vaporize tracking
//! 2. `on_silo_vaporize()` — cancel all pending timers for that Silo

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::timer_wheel::{TimerWheel, TimerId};

#[derive(Debug, Default, Clone)]
pub struct TimerBridgeStats {
    pub scheduled:       u64,
    pub cancelled:       u64,
    pub cleanup_timers:  u64,
}

pub struct TimerWheelSiloBridge {
    pub wheel:       TimerWheel,
    silo_timers:     BTreeMap<u64, Vec<TimerId>>,
    pub stats:       TimerBridgeStats,
}

impl TimerWheelSiloBridge {
    pub fn new(tick_ns: u64) -> Self {
        TimerWheelSiloBridge {
            wheel: TimerWheel::new(tick_ns),
            silo_timers: BTreeMap::new(),
            stats: TimerBridgeStats::default(),
        }
    }

    /// Schedule a one-shot timer for a Silo. Tracks it for vaporize cleanup.
    pub fn schedule_for_silo(&mut self, silo_id: u64, delay_ns: u64, tag: u32) -> TimerId {
        self.stats.scheduled += 1;
        let id = self.wheel.schedule(delay_ns, silo_id, tag);
        self.silo_timers.entry(silo_id).or_default().push(id);
        id
    }

    /// Schedule a repeating timer for a Silo.
    pub fn schedule_repeating_for_silo(&mut self, silo_id: u64, interval_ns: u64, tag: u32) -> TimerId {
        self.stats.scheduled += 1;
        let id = self.wheel.schedule_repeating(interval_ns, silo_id, tag);
        self.silo_timers.entry(silo_id).or_default().push(id);
        id
    }

    /// Cancel all pending timers for a vaporized Silo.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        if let Some(timers) = self.silo_timers.remove(&silo_id) {
            let count = timers.len() as u64;
            for id in timers {
                self.wheel.cancel(id);
                self.stats.cancelled += 1;
            }
            self.stats.cleanup_timers += count;
            crate::serial_println!(
                "[TIMER BRIDGE] Silo {} vaporized: {} timers cancelled", silo_id, count
            );
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TimerBridge: scheduled={} cancelled={} cleanup={}",
            self.stats.scheduled, self.stats.cancelled, self.stats.cleanup_timers
        );
    }
}
