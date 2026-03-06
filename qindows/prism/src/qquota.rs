//! # Q-Quota — Per-Silo Storage Quotas
//!
//! Enforces storage limits per Silo with soft and hard
//! limits (Section 3.13). Prevents any Silo from consuming
//! all available storage.
//!
//! Features:
//! - Hard limit: writes rejected beyond this
//! - Soft limit: warnings emitted but writes allowed
//! - Grace period: soft limit violations given N days before becoming hard
//! - Per-object-type quotas (files, snapshots, versions)
//! - Admin override

extern crate alloc;

use alloc::collections::BTreeMap;

/// Quota type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaType {
    Storage,    // Total bytes
    Objects,    // Number of Q-Objects
    Snapshots,  // Number of snapshots
    Versions,   // Number of versions
}

/// Quota state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaState {
    Ok,
    SoftExceeded,
    HardExceeded,
    GracePeriod,
}

/// A quota entry.
#[derive(Debug, Clone)]
pub struct Quota {
    pub silo_id: u64,
    pub quota_type: QuotaType,
    pub soft_limit: u64,
    pub hard_limit: u64,
    pub current: u64,
    pub state: QuotaState,
    pub grace_expires: u64,
}

/// Quota statistics.
#[derive(Debug, Clone, Default)]
pub struct QuotaStats {
    pub checks: u64,
    pub soft_violations: u64,
    pub hard_violations: u64,
    pub writes_blocked: u64,
}

/// The Q-Quota Manager.
pub struct QQuota {
    /// (silo_id, quota_type) → Quota
    pub quotas: BTreeMap<(u64, u8), Quota>,
    /// Grace period duration (seconds)
    pub grace_duration: u64,
    pub stats: QuotaStats,
}

impl QQuota {
    pub fn new() -> Self {
        QQuota {
            quotas: BTreeMap::new(),
            grace_duration: 86400 * 7, // 7 days
            stats: QuotaStats::default(),
        }
    }

    /// Set quota for a Silo.
    pub fn set(&mut self, silo_id: u64, qt: QuotaType, soft: u64, hard: u64) {
        let key = (silo_id, qt as u8);
        self.quotas.entry(key).or_insert(Quota {
            silo_id, quota_type: qt,
            soft_limit: soft, hard_limit: hard,
            current: 0, state: QuotaState::Ok,
            grace_expires: 0,
        });
        if let Some(q) = self.quotas.get_mut(&key) {
            q.soft_limit = soft;
            q.hard_limit = hard;
        }
    }

    /// Check if a write is allowed and charge usage.
    pub fn charge(&mut self, silo_id: u64, qt: QuotaType, amount: u64, now: u64) -> Result<(), &'static str> {
        let key = (silo_id, qt as u8);
        self.stats.checks += 1;

        let quota = match self.quotas.get_mut(&key) {
            Some(q) => q,
            None => return Ok(()), // No quota set = unlimited
        };

        let new_total = quota.current + amount;

        if new_total > quota.hard_limit {
            quota.state = QuotaState::HardExceeded;
            self.stats.hard_violations += 1;
            self.stats.writes_blocked += 1;
            return Err("Hard quota exceeded");
        }

        if new_total > quota.soft_limit {
            if quota.state == QuotaState::Ok {
                quota.state = QuotaState::GracePeriod;
                quota.grace_expires = now + self.grace_duration;
                self.stats.soft_violations += 1;
            } else if quota.state == QuotaState::GracePeriod && now >= quota.grace_expires {
                quota.state = QuotaState::SoftExceeded;
                self.stats.writes_blocked += 1;
                return Err("Grace period expired");
            }
        }

        quota.current = new_total;
        Ok(())
    }

    /// Release usage.
    pub fn release(&mut self, silo_id: u64, qt: QuotaType, amount: u64) {
        let key = (silo_id, qt as u8);
        if let Some(quota) = self.quotas.get_mut(&key) {
            quota.current = quota.current.saturating_sub(amount);
            if quota.current <= quota.soft_limit {
                quota.state = QuotaState::Ok;
                quota.grace_expires = 0;
            }
        }
    }

    /// Get usage percentage.
    pub fn usage_pct(&self, silo_id: u64, qt: QuotaType) -> f32 {
        let key = (silo_id, qt as u8);
        match self.quotas.get(&key) {
            Some(q) if q.hard_limit > 0 => (q.current as f32 / q.hard_limit as f32) * 100.0,
            _ => 0.0,
        }
    }
}
