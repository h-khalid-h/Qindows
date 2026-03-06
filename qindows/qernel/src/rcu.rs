//! # Read-Copy-Update — Lock-Free Concurrent Reads
//!
//! Provides RCU semantics for kernel data structures that
//! are read-heavy and rarely modified (Section 1.6).
//!
//! Features:
//! - Lock-free reads (no overhead for readers)
//! - Writers create new version, swap atomically
//! - Grace period tracking (safe to free old version after all readers finish)
//! - Callback queue for deferred cleanup
//! - Per-CPU read-side critical sections

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// RCU grace period state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraceState {
    Active,
    Pending,
    Complete,
}

/// An RCU-protected object version.
#[derive(Debug, Clone)]
pub struct RcuVersion {
    pub version: u64,
    pub grace_period: u64,
    pub state: GraceState,
    pub readers: u32,
    pub created_at: u64,
}

/// A deferred cleanup callback.
#[derive(Debug, Clone)]
pub struct DeferredCallback {
    pub id: u64,
    pub grace_period: u64,
    pub object_id: u64,
    pub old_version: u64,
}

/// RCU statistics.
#[derive(Debug, Clone, Default)]
pub struct RcuStats {
    pub reads: u64,
    pub updates: u64,
    pub grace_periods: u64,
    pub callbacks_executed: u64,
    pub callbacks_pending: u64,
}

/// The RCU Manager.
pub struct RcuManager {
    /// Object → current version
    pub current: BTreeMap<u64, u64>,
    /// Version tracking
    pub versions: BTreeMap<u64, RcuVersion>,
    /// Deferred callbacks
    pub callbacks: Vec<DeferredCallback>,
    /// Current grace period number
    pub current_gp: u64,
    /// Per-CPU reader counts
    pub cpu_readers: BTreeMap<u32, u32>,
    next_cb_id: u64,
    pub stats: RcuStats,
}

impl RcuManager {
    pub fn new() -> Self {
        RcuManager {
            current: BTreeMap::new(),
            versions: BTreeMap::new(),
            callbacks: Vec::new(),
            current_gp: 1,
            cpu_readers: BTreeMap::new(),
            next_cb_id: 1,
            stats: RcuStats::default(),
        }
    }

    /// Register a CPU for RCU tracking.
    pub fn register_cpu(&mut self, cpu_id: u32) {
        self.cpu_readers.insert(cpu_id, 0);
    }

    /// Enter read-side critical section.
    pub fn read_lock(&mut self, cpu_id: u32) {
        if let Some(count) = self.cpu_readers.get_mut(&cpu_id) {
            *count += 1;
        }
        self.stats.reads += 1;
    }

    /// Exit read-side critical section.
    pub fn read_unlock(&mut self, cpu_id: u32) {
        if let Some(count) = self.cpu_readers.get_mut(&cpu_id) {
            *count = count.saturating_sub(1);
        }
    }

    /// Publish a new version of an object.
    pub fn publish(&mut self, object_id: u64, now: u64) -> u64 {
        let version = self.current_gp;
        let old_version = self.current.insert(object_id, version);

        self.versions.insert(version, RcuVersion {
            version, grace_period: self.current_gp,
            state: GraceState::Active, readers: 0, created_at: now,
        });

        // Queue old version for cleanup
        if let Some(old_v) = old_version {
            let cb_id = self.next_cb_id;
            self.next_cb_id += 1;
            self.callbacks.push(DeferredCallback {
                id: cb_id, grace_period: self.current_gp,
                object_id, old_version: old_v,
            });
            self.stats.callbacks_pending += 1;
        }

        self.stats.updates += 1;
        version
    }

    /// Start a new grace period.
    pub fn advance_grace_period(&mut self) {
        // Check if all CPUs have passed through a quiescent state
        let all_quiet = self.cpu_readers.values().all(|&c| c == 0);

        if all_quiet {
            // Mark current grace period as complete
            for version in self.versions.values_mut() {
                if version.state == GraceState::Pending {
                    version.state = GraceState::Complete;
                }
                if version.state == GraceState::Active {
                    version.state = GraceState::Pending;
                }
            }

            self.current_gp += 1;
            self.stats.grace_periods += 1;

            // Execute callbacks for completed grace periods
            let completed: Vec<u64> = self.versions.iter()
                .filter(|(_, v)| v.state == GraceState::Complete)
                .map(|(&k, _)| k)
                .collect();

            let before = self.callbacks.len();
            self.callbacks.retain(|cb| !completed.contains(&cb.grace_period));
            let executed = before - self.callbacks.len();
            self.stats.callbacks_executed += executed as u64;
            self.stats.callbacks_pending = self.stats.callbacks_pending.saturating_sub(executed as u64);
        }
    }
}
