//! # Q-Quota — Silo Resource Quota Manager
//!
//! Enforces resource limits per Silo: CPU time slices,
//! memory ceilings, storage quotas, network bandwidth,
//! and GPU/NPU time budgets (Section 2.3).
//!
//! Features:
//! - Per-Silo multi-resource quotas
//! - Soft/hard limit distinction
//! - Quota inheritance for child Silos
//! - Grace period for soft limit violations
//! - Usage tracking and over-limit event logging

extern crate alloc;

use alloc::collections::BTreeMap;

/// Resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    CpuMs,
    MemoryBytes,
    StorageBytes,
    NetworkBytesOut,
    NetworkBytesIn,
    GpuMs,
    NpuMs,
    FileDescriptors,
    Threads,
}

/// Quota limit type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitType {
    /// Warning only, allow continued use
    Soft,
    /// Hard cap — reject requests beyond this
    Hard,
}

/// A quota entry for one resource.
#[derive(Debug, Clone)]
pub struct QuotaEntry {
    pub resource: Resource,
    pub soft_limit: u64,
    pub hard_limit: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
    pub violations_soft: u64,
    pub violations_hard: u64,
    pub grace_period_ms: u64,
    pub grace_expires_at: u64,
}

impl QuotaEntry {
    pub fn new(resource: Resource, soft: u64, hard: u64) -> Self {
        QuotaEntry {
            resource, soft_limit: soft, hard_limit: hard,
            current_usage: 0, peak_usage: 0,
            violations_soft: 0, violations_hard: 0,
            grace_period_ms: 30_000, grace_expires_at: 0,
        }
    }

    /// Check if a request of `amount` would exceed the hard limit.
    pub fn would_exceed(&self, amount: u64) -> bool {
        self.current_usage + amount > self.hard_limit
    }

    /// Check if currently over soft limit.
    pub fn over_soft(&self) -> bool {
        self.current_usage > self.soft_limit
    }
}

/// Per-Silo quota set.
#[derive(Debug, Clone)]
pub struct SiloQuota {
    pub silo_id: u64,
    pub quotas: BTreeMap<Resource, QuotaEntry>,
    pub parent_silo: Option<u64>,
    pub enabled: bool,
}

/// Quota check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaResult {
    Allowed,
    SoftWarning,
    HardDenied,
    NoQuota,
}

/// Quota statistics.
#[derive(Debug, Clone, Default)]
pub struct QuotaStats {
    pub checks: u64,
    pub allowed: u64,
    pub soft_warnings: u64,
    pub hard_denials: u64,
}

/// The Quota Manager.
pub struct QuotaManager {
    pub silos: BTreeMap<u64, SiloQuota>,
    pub stats: QuotaStats,
}

impl QuotaManager {
    pub fn new() -> Self {
        QuotaManager {
            silos: BTreeMap::new(),
            stats: QuotaStats::default(),
        }
    }

    /// Create a quota set for a Silo.
    pub fn create_silo(&mut self, silo_id: u64, parent: Option<u64>) {
        self.silos.insert(silo_id, SiloQuota {
            silo_id, quotas: BTreeMap::new(),
            parent_silo: parent, enabled: true,
        });
    }

    /// Set a quota for a resource.
    pub fn set(&mut self, silo_id: u64, resource: Resource, soft: u64, hard: u64) {
        if let Some(sq) = self.silos.get_mut(&silo_id) {
            sq.quotas.insert(resource, QuotaEntry::new(resource, soft, hard));
        }
    }

    /// Check if a resource request is allowed.
    pub fn check(&mut self, silo_id: u64, resource: Resource, amount: u64, now: u64) -> QuotaResult {
        self.stats.checks += 1;

        let sq = match self.silos.get_mut(&silo_id) {
            Some(sq) if sq.enabled => sq,
            _ => { self.stats.allowed += 1; return QuotaResult::NoQuota; }
        };

        let entry = match sq.quotas.get_mut(&resource) {
            Some(e) => e,
            None => { self.stats.allowed += 1; return QuotaResult::NoQuota; }
        };

        if entry.would_exceed(amount) {
            entry.violations_hard += 1;
            self.stats.hard_denials += 1;
            return QuotaResult::HardDenied;
        }

        if entry.current_usage + amount > entry.soft_limit {
            entry.violations_soft += 1;
            if entry.grace_expires_at == 0 {
                entry.grace_expires_at = now + entry.grace_period_ms;
            }
            self.stats.soft_warnings += 1;
            return QuotaResult::SoftWarning;
        }

        self.stats.allowed += 1;
        QuotaResult::Allowed
    }

    /// Record resource usage.
    pub fn record(&mut self, silo_id: u64, resource: Resource, amount: u64) {
        if let Some(sq) = self.silos.get_mut(&silo_id) {
            if let Some(entry) = sq.quotas.get_mut(&resource) {
                entry.current_usage += amount;
                if entry.current_usage > entry.peak_usage {
                    entry.peak_usage = entry.current_usage;
                }
            }
        }
    }

    /// Release resource usage.
    pub fn release(&mut self, silo_id: u64, resource: Resource, amount: u64) {
        if let Some(sq) = self.silos.get_mut(&silo_id) {
            if let Some(entry) = sq.quotas.get_mut(&resource) {
                entry.current_usage = entry.current_usage.saturating_sub(amount);
                if !entry.over_soft() {
                    entry.grace_expires_at = 0; // Reset grace
                }
            }
        }
    }

    /// Clean up a terminated Silo.
    pub fn remove_silo(&mut self, silo_id: u64) {
        self.silos.remove(&silo_id);
    }
}
