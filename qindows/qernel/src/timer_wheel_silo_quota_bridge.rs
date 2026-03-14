//! # Timer Wheel Silo Quota Bridge (Phase 192)
//!
//! ## Architecture Guardian: The Gap
//! `timer_wheel.rs` implements `TimerWheel`:
//! - `TimerWheel::schedule(delay_ns, silo_id, tag)` → TimerId
//! - `TimerWheel::schedule_repeating(interval_ns, silo_id, tag)` → TimerId
//! - `TimerWheel::cancel(id: TimerId)` → bool
//!
//! **Missing link**: A Silo could schedule unlimited timers — causing timer
//! table overflow and blocking other Silos from scheduling. No quota enforced.
//!
//! This module provides `TimerWheelSiloQuotaBridge`:
//! Max 32 live timers per Silo. Exceeding the quota silently fails.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::timer_wheel::{TimerWheel, TimerId};

const MAX_TIMERS_PER_SILO: usize = 32;

#[derive(Debug, Default, Clone)]
pub struct TimerQuotaStats {
    pub scheduled:     u64,
    pub quota_denied:  u64,
    pub cancelled:     u64,
}

pub struct TimerWheelSiloQuotaBridge {
    pub wheel:     TimerWheel,
    silo_counts:   BTreeMap<u64, usize>,
    pub stats:     TimerQuotaStats,
}

impl TimerWheelSiloQuotaBridge {
    pub fn new(tick_ns: u64) -> Self {
        TimerWheelSiloQuotaBridge {
            wheel: TimerWheel::new(tick_ns),
            silo_counts: BTreeMap::new(),
            stats: TimerQuotaStats::default(),
        }
    }

    /// Schedule a one-shot timer — capped at 32 per Silo.
    pub fn schedule(
        &mut self,
        silo_id: u64,
        delay_ns: u64,
        tag: u32,
    ) -> Option<TimerId> {
        let count = self.silo_counts.entry(silo_id).or_default();
        if *count >= MAX_TIMERS_PER_SILO {
            self.stats.quota_denied += 1;
            crate::serial_println!(
                "[TIMER] Silo {} quota exceeded: {}/{} timers", silo_id, count, MAX_TIMERS_PER_SILO
            );
            return None;
        }
        *count += 1;
        self.stats.scheduled += 1;
        Some(self.wheel.schedule(delay_ns, silo_id, tag))
    }

    /// Cancel a timer and free the quota slot.
    pub fn cancel(&mut self, silo_id: u64, id: TimerId) -> bool {
        if self.wheel.cancel(id) {
            self.stats.cancelled += 1;
            if let Some(count) = self.silo_counts.get_mut(&silo_id) {
                *count = count.saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TimerQuotaBridge: scheduled={} denied={} cancelled={}",
            self.stats.scheduled, self.stats.quota_denied, self.stats.cancelled
        );
    }
}
