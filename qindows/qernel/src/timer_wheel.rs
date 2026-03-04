//! # Qernel Hierarchical Timer Wheel
//!
//! Efficient timer management using a hierarchical timing wheel.
//! Supports scheduling callbacks at nanosecond precision with
//! O(1) insertion and amortized O(1) expiration.

extern crate alloc;

use alloc::vec::Vec;

/// Number of slots per wheel level.
const WHEEL_SIZE: usize = 256;
/// Number of wheel levels (nanosecond granularity at level 0).
const WHEEL_LEVELS: usize = 4;

/// A timer callback identifier.
pub type TimerId = u64;

/// Timer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    /// Waiting to fire
    Pending,
    /// Currently firing
    Firing,
    /// Cancelled
    Cancelled,
    /// Already fired
    Expired,
}

/// A scheduled timer.
#[derive(Debug, Clone)]
pub struct Timer {
    /// Unique timer ID
    pub id: TimerId,
    /// Absolute expiration time (ns since boot)
    pub expires_at: u64,
    /// State
    pub state: TimerState,
    /// Repeat interval (0 = one-shot)
    pub interval_ns: u64,
    /// Owning Silo ID (0 = kernel)
    pub silo_id: u64,
    /// Callback tag (passed to handler)
    pub tag: u32,
}

/// A single wheel level.
struct WheelLevel {
    /// Slots, each containing a list of timer IDs
    slots: [Vec<TimerId>; WHEEL_SIZE],
    /// Current slot index
    current: usize,
}

impl WheelLevel {
    fn new() -> Self {
        WheelLevel {
            slots: core::array::from_fn(|_| Vec::new()),
            current: 0,
        }
    }
}

/// The Hierarchical Timer Wheel.
pub struct TimerWheel {
    /// Wheel levels (level 0 = finest granularity)
    levels: [WheelLevel; WHEEL_LEVELS],
    /// All timers by ID
    timers: alloc::collections::BTreeMap<TimerId, Timer>,
    /// Tick resolution (ns per level-0 slot)
    pub tick_ns: u64,
    /// Current time (ns)
    pub now_ns: u64,
    /// Next timer ID
    next_id: TimerId,
    /// Expired timer IDs (ready to fire)
    pub expired: Vec<TimerId>,
    /// Stats
    pub stats: TimerStats,
}

/// Timer statistics.
#[derive(Debug, Clone, Default)]
pub struct TimerStats {
    pub timers_created: u64,
    pub timers_fired: u64,
    pub timers_cancelled: u64,
    pub timers_rescheduled: u64,
    pub ticks_processed: u64,
}

impl TimerWheel {
    /// Create a new timer wheel.
    ///
    /// `tick_ns` is the resolution of each level-0 slot.
    /// Level 1 has `tick_ns * 256` per slot, level 2 has `tick_ns * 256²`, etc.
    pub fn new(tick_ns: u64) -> Self {
        TimerWheel {
            levels: core::array::from_fn(|_| WheelLevel::new()),
            timers: alloc::collections::BTreeMap::new(),
            tick_ns,
            now_ns: 0,
            next_id: 1,
            expired: Vec::new(),
            stats: TimerStats::default(),
        }
    }

    /// Schedule a one-shot timer.
    pub fn schedule(&mut self, delay_ns: u64, silo_id: u64, tag: u32) -> TimerId {
        let id = self.next_id;
        self.next_id += 1;

        let expires_at = self.now_ns + delay_ns;
        let timer = Timer {
            id,
            expires_at,
            state: TimerState::Pending,
            interval_ns: 0,
            silo_id,
            tag,
        };

        self.insert_timer(&timer);
        self.timers.insert(id, timer);
        self.stats.timers_created += 1;

        id
    }

    /// Schedule a repeating timer.
    pub fn schedule_repeating(&mut self, interval_ns: u64, silo_id: u64, tag: u32) -> TimerId {
        let id = self.next_id;
        self.next_id += 1;

        let expires_at = self.now_ns + interval_ns;
        let timer = Timer {
            id,
            expires_at,
            state: TimerState::Pending,
            interval_ns,
            silo_id,
            tag,
        };

        self.insert_timer(&timer);
        self.timers.insert(id, timer);
        self.stats.timers_created += 1;

        id
    }

    /// Cancel a timer.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        if let Some(timer) = self.timers.get_mut(&id) {
            if timer.state == TimerState::Pending {
                timer.state = TimerState::Cancelled;
                self.stats.timers_cancelled += 1;
                return true;
            }
        }
        false
    }

    /// Advance the wheel by one tick.
    pub fn tick(&mut self) {
        self.now_ns += self.tick_ns;
        self.stats.ticks_processed += 1;
        self.expired.clear();

        // Process level 0
        self.levels[0].current = (self.levels[0].current + 1) % WHEEL_SIZE;

        // Cascade from higher levels when level wraps
        for level in 1..WHEEL_LEVELS {
            if self.levels[level - 1].current == 0 {
                self.levels[level].current = (self.levels[level].current + 1) % WHEEL_SIZE;
                // Move timers from this level down to level 0
                let timers_to_redistribute: Vec<TimerId> =
                    self.levels[level].slots[self.levels[level].current].drain(..).collect();

                for timer_id in timers_to_redistribute {
                    if let Some(timer) = self.timers.get(&timer_id) {
                        if timer.state == TimerState::Pending {
                            let timer_clone = timer.clone();
                            self.insert_timer(&timer_clone);
                        }
                    }
                }
            } else {
                break; // No cascade needed
            }
        }

        // Collect expired timers from level 0's current slot
        let expired_ids: Vec<TimerId> =
            self.levels[0].slots[self.levels[0].current].drain(..).collect();

        for id in expired_ids {
            if let Some(timer) = self.timers.get_mut(&id) {
                if timer.state == TimerState::Cancelled { continue; }

                timer.state = TimerState::Firing;
                self.expired.push(id);
                self.stats.timers_fired += 1;

                if timer.interval_ns > 0 {
                    // Repeating timer — reschedule
                    let new_expires = self.now_ns + timer.interval_ns;
                    timer.expires_at = new_expires;
                    timer.state = TimerState::Pending;
                    let timer_clone = timer.clone();
                    self.insert_timer(&timer_clone);
                    self.stats.timers_rescheduled += 1;
                } else {
                    timer.state = TimerState::Expired;
                }
            }
        }
    }

    /// Advance time to `target_ns`, processing all ticks.
    pub fn advance_to(&mut self, target_ns: u64) {
        while self.now_ns < target_ns {
            self.tick();
        }
    }

    /// Insert a timer into the correct wheel slot.
    fn insert_timer(&mut self, timer: &Timer) {
        let delta = if timer.expires_at > self.now_ns {
            timer.expires_at - self.now_ns
        } else {
            0
        };
        let delta_ticks = delta / self.tick_ns;

        // Find the right level
        let (level, slot) = if delta_ticks < WHEEL_SIZE as u64 {
            (0, (self.levels[0].current + delta_ticks as usize) % WHEEL_SIZE)
        } else if delta_ticks < (WHEEL_SIZE * WHEEL_SIZE) as u64 {
            let l1_offset = delta_ticks as usize / WHEEL_SIZE;
            (1, (self.levels[1].current + l1_offset) % WHEEL_SIZE)
        } else if delta_ticks < (WHEEL_SIZE * WHEEL_SIZE * WHEEL_SIZE) as u64 {
            let l2_offset = delta_ticks as usize / (WHEEL_SIZE * WHEEL_SIZE);
            (2, (self.levels[2].current + l2_offset) % WHEEL_SIZE)
        } else {
            let l3_offset = (delta_ticks as usize / (WHEEL_SIZE * WHEEL_SIZE * WHEEL_SIZE))
                .min(WHEEL_SIZE - 1);
            (3, (self.levels[3].current + l3_offset) % WHEEL_SIZE)
        };

        self.levels[level].slots[slot].push(timer.id);
    }

    /// Number of pending timers.
    pub fn pending_count(&self) -> usize {
        self.timers.values().filter(|t| t.state == TimerState::Pending).count()
    }

    /// Cancel all timers for a Silo.
    pub fn cancel_for_silo(&mut self, silo_id: u64) {
        for timer in self.timers.values_mut() {
            if timer.silo_id == silo_id && timer.state == TimerState::Pending {
                timer.state = TimerState::Cancelled;
                self.stats.timers_cancelled += 1;
            }
        }
    }

    /// Clean up expired and cancelled timers.
    pub fn cleanup(&mut self) {
        self.timers.retain(|_, t| t.state == TimerState::Pending);
    }
}
