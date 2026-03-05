//! # Q-Lock — Distributed File Locking
//!
//! Provides advisory and mandatory file locks across Silos and
//! mesh peers (Section 3.6). Prevents data corruption when
//! multiple Silos access the same Q-Object.
//!
//! Features:
//! - Shared (read) and exclusive (write) locks
//! - Lock promotion (shared → exclusive)
//! - Deadlock detection via wait-for graph
//! - Lease-based expiry for crash recovery

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Lock type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    Shared,    // Multiple readers allowed
    Exclusive, // Single writer only
}

/// Lock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Held,
    Waiting,
    Expired,
    Released,
}

/// A file lock.
#[derive(Debug, Clone)]
pub struct FileLock {
    pub id: u64,
    pub oid: u64,
    pub silo_id: u64,
    pub lock_type: LockType,
    pub state: LockState,
    pub acquired_at: u64,
    pub expires_at: u64,
    pub byte_start: u64,
    pub byte_end: u64,
}

/// Lock statistics.
#[derive(Debug, Clone, Default)]
pub struct LockStats {
    pub locks_acquired: u64,
    pub locks_released: u64,
    pub locks_expired: u64,
    pub contentions: u64,
    pub deadlocks_detected: u64,
    pub promotions: u64,
}

/// The Q-Lock Manager.
pub struct QLock {
    pub locks: BTreeMap<u64, FileLock>,
    /// Wait-for graph: silo → set of silos it's waiting on
    pub wait_for: BTreeMap<u64, Vec<u64>>,
    next_id: u64,
    /// Default lease duration (seconds)
    pub lease_duration: u64,
    pub stats: LockStats,
}

impl QLock {
    pub fn new() -> Self {
        QLock {
            locks: BTreeMap::new(),
            wait_for: BTreeMap::new(),
            next_id: 1,
            lease_duration: 30,
            stats: LockStats::default(),
        }
    }

    /// Acquire a lock on a byte range of an object.
    pub fn acquire(&mut self, oid: u64, silo_id: u64, lock_type: LockType, byte_start: u64, byte_end: u64, now: u64) -> Result<u64, &'static str> {
        // Check for conflicts
        let conflicts: Vec<u64> = self.locks.values()
            .filter(|l| l.oid == oid && l.state == LockState::Held && l.silo_id != silo_id)
            .filter(|l| l.byte_start < byte_end && l.byte_end > byte_start)
            .filter(|l| lock_type == LockType::Exclusive || l.lock_type == LockType::Exclusive)
            .map(|l| l.silo_id)
            .collect();

        if !conflicts.is_empty() {
            self.stats.contentions += 1;

            // Add to wait-for graph
            self.wait_for.entry(silo_id).or_insert_with(Vec::new).extend(conflicts.iter());

            // Check for deadlock
            if self.detect_deadlock(silo_id) {
                self.wait_for.remove(&silo_id);
                self.stats.deadlocks_detected += 1;
                return Err("Deadlock detected");
            }

            return Err("Lock conflict");
        }

        let id = self.next_id;
        self.next_id += 1;

        self.locks.insert(id, FileLock {
            id, oid, silo_id, lock_type,
            state: LockState::Held,
            acquired_at: now,
            expires_at: now + self.lease_duration,
            byte_start, byte_end,
        });

        // Clear wait-for since we acquired
        self.wait_for.remove(&silo_id);
        self.stats.locks_acquired += 1;
        Ok(id)
    }

    /// Release a lock.
    pub fn release(&mut self, lock_id: u64) {
        if let Some(lock) = self.locks.get_mut(&lock_id) {
            lock.state = LockState::Released;
            self.stats.locks_released += 1;
        }
    }

    /// Promote a shared lock to exclusive.
    pub fn promote(&mut self, lock_id: u64) -> Result<(), &'static str> {
        let lock = self.locks.get(&lock_id).ok_or("Lock not found")?;
        if lock.lock_type != LockType::Shared || lock.state != LockState::Held {
            return Err("Can only promote held shared locks");
        }

        let oid = lock.oid;
        let silo_id = lock.silo_id;
        let byte_start = lock.byte_start;
        let byte_end = lock.byte_end;

        // Check for other shared locks on this range
        let other_shared = self.locks.values()
            .any(|l| l.oid == oid && l.id != lock_id && l.state == LockState::Held
                && l.byte_start < byte_end && l.byte_end > byte_start
                && l.silo_id != silo_id);

        if other_shared {
            return Err("Other shared locks exist on range");
        }

        if let Some(lock) = self.locks.get_mut(&lock_id) {
            lock.lock_type = LockType::Exclusive;
            self.stats.promotions += 1;
        }
        Ok(())
    }

    /// Expire stale locks.
    pub fn expire(&mut self, now: u64) {
        for lock in self.locks.values_mut() {
            if lock.state == LockState::Held && now >= lock.expires_at {
                lock.state = LockState::Expired;
                self.stats.locks_expired += 1;
            }
        }
    }

    /// Simple deadlock detection (cycle in wait-for graph).
    fn detect_deadlock(&self, start: u64) -> bool {
        let mut visited = Vec::new();
        let mut stack = vec![start];

        while let Some(node) = stack.pop() {
            if visited.contains(&node) { continue; }
            visited.push(node);

            if let Some(waiting_on) = self.wait_for.get(&node) {
                for &target in waiting_on {
                    if target == start { return true; }
                    stack.push(target);
                }
            }
        }
        false
    }
}
