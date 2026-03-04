//! # Qernel Timer Subsystem
//!
//! Provides time measurement, scheduling deadlines, and sleep functionality.
//! Uses APIC timer ticks internally, calibrated against the PIT or TSC
//! for accurate wall-clock time.

pub mod hpet;

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrd};

/// Global tick counter — incremented by the APIC timer interrupt.
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Ticks per second (calibrated during boot).
static TICKS_PER_SECOND: AtomicU64 = AtomicU64::new(100);

/// Increment the tick counter (called from the timer interrupt handler).
pub fn tick() {
    TICK_COUNT.fetch_add(1, AtomicOrd::Relaxed);
}

/// Get the current tick count.
pub fn now_ticks() -> u64 {
    TICK_COUNT.load(AtomicOrd::Relaxed)
}

/// Get uptime in milliseconds.
pub fn uptime_ms() -> u64 {
    let ticks = now_ticks();
    let tps = TICKS_PER_SECOND.load(AtomicOrd::Relaxed);
    if tps == 0 { return 0; }
    ticks * 1000 / tps
}

/// Get uptime in seconds.
pub fn uptime_seconds() -> u64 {
    let ticks = now_ticks();
    let tps = TICKS_PER_SECOND.load(AtomicOrd::Relaxed);
    if tps == 0 { return 0; }
    ticks / tps
}

/// Set the ticks-per-second calibration value.
pub fn set_frequency(tps: u64) {
    TICKS_PER_SECOND.store(tps, AtomicOrd::Relaxed);
}

/// A scheduled timer event.
#[derive(Debug, Clone)]
pub struct TimerEvent {
    /// When this timer fires (absolute tick count)
    pub fire_at: u64,
    /// Timer identifier
    pub id: u64,
    /// What to do when fired
    pub action: TimerAction,
    /// Is this a repeating timer?
    pub repeat_interval: Option<u64>,
}

/// Timer actions
#[derive(Debug, Clone, Copy)]
pub enum TimerAction {
    /// Wake a sleeping fiber
    WakeFiber(u64),
    /// Fire a Sentinel health check
    SentinelScan,
    /// Send a heartbeat to the mesh
    MeshHeartbeat,
    /// Update power manager statistics
    PowerUpdate,
    /// Flush the Prism journal to disk
    PrismFlush,
    /// Run the scheduler's load balancer
    LoadBalance,
    /// Custom callback (function pointer as u64)
    Custom(u64),
}

// Implement ordering for BinaryHeap (min-heap by fire_at)
impl PartialEq for TimerEvent {
    fn eq(&self, other: &Self) -> bool { self.fire_at == other.fire_at }
}
impl Eq for TimerEvent {}
impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (BinaryHeap is max-heap by default)
        other.fire_at.cmp(&self.fire_at)
    }
}

/// The Timer Queue — manages all scheduled events.
pub struct TimerQueue {
    /// Priority queue of pending timer events (min-heap)
    heap: BinaryHeap<TimerEvent>,
    /// Next timer ID
    next_id: u64,
}

impl TimerQueue {
    pub const fn new() -> Self {
        TimerQueue {
            heap: BinaryHeap::new(),
            next_id: 1,
        }
    }

    /// Schedule a one-shot timer.
    pub fn schedule(&mut self, delay_ticks: u64, action: TimerAction) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.heap.push(TimerEvent {
            fire_at: now_ticks() + delay_ticks,
            id,
            action,
            repeat_interval: None,
        });

        id
    }

    /// Schedule a repeating timer.
    pub fn schedule_repeating(&mut self, interval_ticks: u64, action: TimerAction) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.heap.push(TimerEvent {
            fire_at: now_ticks() + interval_ticks,
            id,
            action,
            repeat_interval: Some(interval_ticks),
        });

        id
    }

    /// Cancel a timer by ID.
    pub fn cancel(&mut self, id: u64) {
        // Rebuild the heap without the cancelled timer
        let events: Vec<TimerEvent> = self.heap.drain().filter(|e| e.id != id).collect();
        for event in events {
            self.heap.push(event);
        }
    }

    /// Process all expired timers.
    ///
    /// Called from the APIC timer interrupt handler.
    /// Returns a list of actions that need to be executed.
    pub fn process_expired(&mut self) -> Vec<TimerAction> {
        let now = now_ticks();
        let mut actions = Vec::new();

        while let Some(event) = self.heap.peek() {
            if event.fire_at > now {
                break; // All remaining timers are in the future
            }

            let event = self.heap.pop().unwrap();
            actions.push(event.action);

            // Reschedule repeating timers
            if let Some(interval) = event.repeat_interval {
                self.heap.push(TimerEvent {
                    fire_at: now + interval,
                    id: event.id,
                    action: event.action,
                    repeat_interval: Some(interval),
                });
            }
        }

        actions
    }

    /// Get the number of pending timers.
    pub fn pending_count(&self) -> usize {
        self.heap.len()
    }
}

/// Initialize the timer subsystem with default system timers.
pub fn init() -> TimerQueue {
    let mut queue = TimerQueue::new();
    let tps = TICKS_PER_SECOND.load(AtomicOrd::Relaxed);

    // Sentinel health scan every 5 seconds
    queue.schedule_repeating(tps * 5, TimerAction::SentinelScan);

    // Mesh heartbeat every 30 seconds
    queue.schedule_repeating(tps * 30, TimerAction::MeshHeartbeat);

    // Power stats update every 1 second
    queue.schedule_repeating(tps, TimerAction::PowerUpdate);

    // Prism journal flush every 10 seconds
    queue.schedule_repeating(tps * 10, TimerAction::PrismFlush);

    // Load balancer every 100ms
    queue.schedule_repeating(tps / 10, TimerAction::LoadBalance);

    crate::serial_println!("[OK] Timer queue initialized ({} system timers)", queue.pending_count());
    queue
}
