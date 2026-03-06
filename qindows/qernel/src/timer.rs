//! # High-Resolution Timer — Per-Core hrtimer + Deadline Scheduling
//!
//! Provides high-resolution timers for precise kernel and
//! userspace timing (Section 9.11).
//!
//! Features:
//! - Per-core timer wheel
//! - Nanosecond-resolution deadlines
//! - One-shot and periodic modes
//! - Callback-based expiry
//! - Timer coalescing for power efficiency

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Timer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    OneShot,
    Periodic,
}

/// Timer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Pending,
    Expired,
    Cancelled,
}

/// A high-resolution timer.
#[derive(Debug, Clone)]
pub struct HrTimer {
    pub id: u64,
    pub core_id: u32,
    pub deadline_ns: u64,
    pub interval_ns: u64,
    pub mode: TimerMode,
    pub state: TimerState,
    pub silo_id: u64,
    pub callback_id: u64,
    pub fires: u64,
}

/// Timer statistics.
#[derive(Debug, Clone, Default)]
pub struct TimerStats {
    pub timers_created: u64,
    pub timers_expired: u64,
    pub timers_cancelled: u64,
    pub coalesced: u64,
}

/// The High-Resolution Timer Manager.
pub struct TimerManager {
    /// Timers sorted by deadline: deadline_ns → timer
    pub timers: BTreeMap<u64, Vec<HrTimer>>,
    next_id: u64,
    /// Coalescing window (ns) — merge timers within this window
    pub coalesce_window_ns: u64,
    pub stats: TimerStats,
}

impl TimerManager {
    pub fn new() -> Self {
        TimerManager {
            timers: BTreeMap::new(),
            next_id: 1,
            coalesce_window_ns: 1_000_000, // 1ms default
            stats: TimerStats::default(),
        }
    }

    /// Create a one-shot timer.
    pub fn set_oneshot(&mut self, core_id: u32, silo_id: u64, deadline_ns: u64, callback_id: u64) -> u64 {
        self.create_timer(core_id, silo_id, deadline_ns, 0, TimerMode::OneShot, callback_id)
    }

    /// Create a periodic timer.
    pub fn set_periodic(&mut self, core_id: u32, silo_id: u64, interval_ns: u64, callback_id: u64, now: u64) -> u64 {
        let deadline = now + interval_ns;
        self.create_timer(core_id, silo_id, deadline, interval_ns, TimerMode::Periodic, callback_id)
    }

    fn create_timer(&mut self, core_id: u32, silo_id: u64, deadline_ns: u64, interval_ns: u64, mode: TimerMode, callback_id: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        // Coalesce: snap deadline to nearest coalesce boundary
        let coalesced_deadline = if self.coalesce_window_ns > 0 {
            let window = self.coalesce_window_ns;
            ((deadline_ns + window - 1) / window) * window
        } else {
            deadline_ns
        };

        if coalesced_deadline != deadline_ns {
            self.stats.coalesced += 1;
        }

        let timer = HrTimer {
            id, core_id, deadline_ns: coalesced_deadline, interval_ns,
            mode, state: TimerState::Pending, silo_id, callback_id, fires: 0,
        };

        self.timers.entry(coalesced_deadline).or_insert_with(Vec::new).push(timer);
        self.stats.timers_created += 1;
        id
    }

    /// Process expired timers. Returns callback IDs to fire.
    pub fn tick(&mut self, now_ns: u64) -> Vec<u64> {
        let mut callbacks = Vec::new();
        let mut reinserts = Vec::new();

        // Collect all expired deadlines
        let expired_deadlines: Vec<u64> = self.timers.range(..=now_ns)
            .map(|(&k, _)| k)
            .collect();

        for deadline in expired_deadlines {
            if let Some(mut timers) = self.timers.remove(&deadline) {
                for timer in &mut timers {
                    if timer.state != TimerState::Pending { continue; }
                    timer.state = TimerState::Expired;
                    timer.fires += 1;
                    callbacks.push(timer.callback_id);
                    self.stats.timers_expired += 1;

                    // Re-arm periodic timers
                    if timer.mode == TimerMode::Periodic {
                        let mut next = timer.clone();
                        next.deadline_ns = now_ns + next.interval_ns;
                        next.state = TimerState::Pending;
                        reinserts.push(next);
                    }
                }
            }
        }

        // Reinsert periodic timers
        for timer in reinserts {
            self.timers.entry(timer.deadline_ns).or_insert_with(Vec::new).push(timer);
        }

        callbacks
    }

    /// Cancel a timer by ID.
    pub fn cancel(&mut self, timer_id: u64) -> bool {
        for timers in self.timers.values_mut() {
            if let Some(t) = timers.iter_mut().find(|t| t.id == timer_id) {
                t.state = TimerState::Cancelled;
                self.stats.timers_cancelled += 1;
                return true;
            }
        }
        false
    }
}
