//! # Ticket Spinlock — Fair, Bounded-Wait Locking Primitive
//!
//! A ticket-based spinlock providing FIFO fairness for
//! kernel-internal mutual exclusion (Section 9.12).
//!
//! Features:
//! - FIFO ordering (no starvation)
//! - Per-lock contention statistics
//! - Spin-count tracking for perf analysis
//! - Owner tracking for deadlock detection
//! - Interrupt-safe variants

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

/// A ticket spinlock.
pub struct TicketLock {
    now_serving: AtomicU64,
    next_ticket: AtomicU64,
    owner_core: AtomicU64,
    pub stats: LockStats,
}

/// Lock statistics.
#[derive(Debug, Clone, Default)]
pub struct LockStats {
    pub acquires: u64,
    pub releases: u64,
    pub total_spins: u64,
    pub max_spins: u64,
    pub contentions: u64,
}

impl TicketLock {
    pub const fn new() -> Self {
        TicketLock {
            now_serving: AtomicU64::new(0),
            next_ticket: AtomicU64::new(0),
            owner_core: AtomicU64::new(u64::MAX),
            stats: LockStats {
                acquires: 0, releases: 0, total_spins: 0,
                max_spins: 0, contentions: 0,
            },
        }
    }

    /// Acquire the lock. Returns the ticket number.
    pub fn lock(&mut self, core_id: u64) -> u64 {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        let mut spins: u64 = 0;

        while self.now_serving.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
            spins += 1;
        }

        self.owner_core.store(core_id, Ordering::Relaxed);
        self.stats.acquires += 1;
        self.stats.total_spins += spins;
        if spins > self.stats.max_spins {
            self.stats.max_spins = spins;
        }
        if spins > 0 {
            self.stats.contentions += 1;
        }

        ticket
    }

    /// Release the lock.
    pub fn unlock(&mut self) {
        self.owner_core.store(u64::MAX, Ordering::Relaxed);
        self.now_serving.fetch_add(1, Ordering::Release);
        self.stats.releases += 1;
    }

    /// Try to acquire the lock without spinning.
    pub fn try_lock(&mut self, core_id: u64) -> Option<u64> {
        let current = self.now_serving.load(Ordering::Relaxed);
        let next = self.next_ticket.load(Ordering::Relaxed);

        // Only acquire if no one is waiting
        if current == next {
            let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
            if self.now_serving.load(Ordering::Acquire) == ticket {
                self.owner_core.store(core_id, Ordering::Relaxed);
                self.stats.acquires += 1;
                return Some(ticket);
            }
            // Lost the race — give back ticket by serving it
            self.now_serving.fetch_add(1, Ordering::Release);
        }
        None
    }

    /// Check if locked (for debugging).
    pub fn is_locked(&self) -> bool {
        self.now_serving.load(Ordering::Relaxed) != self.next_ticket.load(Ordering::Relaxed)
    }

    /// Get the current owner core ID.
    pub fn owner(&self) -> Option<u64> {
        let o = self.owner_core.load(Ordering::Relaxed);
        if o == u64::MAX { None } else { Some(o) }
    }
}
