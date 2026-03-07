//! # Prism Storage Quota System
//!
//! Enforces per-Silo storage limits. Each Silo has a quota
//! (from its app manifest) and the quota system tracks usage,
//! enforces limits, and provides usage reports.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A storage quota for a Silo.
#[derive(Debug, Clone)]
pub struct Quota {
    /// Silo ID
    pub silo_id: u64,
    /// App name
    pub app_name: String,
    /// Maximum storage allowed (bytes)
    pub limit: u64,
    /// Current usage (bytes)
    pub used: u64,
    /// Number of objects stored
    pub object_count: u64,
    /// Largest single object (bytes)
    pub largest_object: u64,
    /// Is this silo over quota?
    pub over_quota: bool,
    /// Grace period remaining (seconds, 0 = hard limit reached)
    pub grace_seconds: u64,
}

impl Quota {
    /// Available space.
    pub fn available(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }

    /// Usage percentage.
    pub fn usage_percent(&self) -> f32 {
        if self.limit == 0 { return 100.0; }
        (self.used as f64 / self.limit as f64 * 100.0) as f32
    }
}

/// Quota enforcement policy.
#[derive(Debug, Clone, Copy)]
pub enum EnforcementPolicy {
    /// Hard limit — reject writes immediately
    Hard,
    /// Soft limit with grace period
    Soft { grace_seconds: u64 },
    /// Warn only (no enforcement)
    WarnOnly,
}

/// Quota check result.
#[derive(Debug, Clone)]
pub enum QuotaCheck {
    /// Write is allowed
    Allowed,
    /// Write is allowed but close to limit
    Warning { usage_percent: f32 },
    /// Write denied — over quota
    Denied { over_by: u64 },
    /// Write allowed during grace period
    GracePeriod { remaining_seconds: u64 },
}

/// Quota event for logging.
#[derive(Debug, Clone)]
pub enum QuotaEvent {
    /// Quota exceeded
    Exceeded { silo_id: u64, used: u64, limit: u64 },
    /// Approaching limit (>80%)
    Warning { silo_id: u64, usage_percent: f32 },
    /// Quota increased
    Increased { silo_id: u64, old: u64, new: u64 },
    /// Grace period expired
    GraceExpired { silo_id: u64 },
}

/// The Quota Manager.
pub struct QuotaManager {
    /// Per-silo quotas
    pub quotas: BTreeMap<u64, Quota>,
    /// Default quota for new silos
    pub default_limit: u64,
    /// Enforcement policy
    pub policy: EnforcementPolicy,
    /// System-wide storage used
    pub total_used: u64,
    /// System-wide storage capacity
    pub total_capacity: u64,
    /// Event log
    pub events: Vec<QuotaEvent>,
    /// Max events to keep
    pub max_events: usize,
}

impl QuotaManager {
    pub fn new(total_capacity: u64) -> Self {
        QuotaManager {
            quotas: BTreeMap::new(),
            default_limit: 100 * 1024 * 1024, // 100 MiB
            policy: EnforcementPolicy::Soft { grace_seconds: 3600 },
            total_used: 0,
            total_capacity,
            events: Vec::new(),
            max_events: 500,
        }
    }

    /// Register a new silo with a quota.
    pub fn register(&mut self, silo_id: u64, app_name: &str, limit: u64) {
        self.quotas.insert(silo_id, Quota {
            silo_id,
            app_name: String::from(app_name),
            limit,
            used: 0,
            object_count: 0,
            largest_object: 0,
            over_quota: false,
            grace_seconds: 0,
        });
    }

    /// Check if a write of `size` bytes is allowed for a silo.
    pub fn check_write(&self, silo_id: u64, size: u64) -> QuotaCheck {
        let quota = match self.quotas.get(&silo_id) {
            Some(q) => q,
            None => return QuotaCheck::Allowed, // Unknown silo = no quota
        };

        let new_used = quota.used + size;

        if new_used <= quota.limit {
            let pct = (new_used as f64 / quota.limit as f64 * 100.0) as f32;
            if pct > 80.0 {
                QuotaCheck::Warning { usage_percent: pct }
            } else {
                QuotaCheck::Allowed
            }
        } else {
            match self.policy {
                EnforcementPolicy::Hard => QuotaCheck::Denied { over_by: new_used - quota.limit },
                EnforcementPolicy::Soft { grace_seconds } => {
                    if quota.grace_seconds > 0 {
                        QuotaCheck::GracePeriod { remaining_seconds: quota.grace_seconds }
                    } else if !quota.over_quota {
                        QuotaCheck::GracePeriod { remaining_seconds: grace_seconds }
                    } else {
                        QuotaCheck::Denied { over_by: new_used - quota.limit }
                    }
                }
                EnforcementPolicy::WarnOnly => QuotaCheck::Warning {
                    usage_percent: (new_used as f64 / quota.limit as f64 * 100.0) as f32,
                },
            }
        }
    }

    /// Record a write (update usage).
    pub fn record_write(&mut self, silo_id: u64, size: u64) {
        let mut event: Option<QuotaEvent> = None;

        if let Some(quota) = self.quotas.get_mut(&silo_id) {
            quota.used += size;
            quota.object_count += 1;
            if size > quota.largest_object {
                quota.largest_object = size;
            }

            if quota.used > quota.limit && !quota.over_quota {
                quota.over_quota = true;
                if let EnforcementPolicy::Soft { grace_seconds } = self.policy {
                    quota.grace_seconds = grace_seconds;
                }
                event = Some(QuotaEvent::Exceeded {
                    silo_id, used: quota.used, limit: quota.limit,
                });
            } else if quota.usage_percent() > 80.0 {
                event = Some(QuotaEvent::Warning {
                    silo_id, usage_percent: quota.usage_percent(),
                });
            }

            self.total_used += size;
        }

        if let Some(evt) = event {
            self.log_event(evt);
        }
    }

    /// Record a deletion (free space).
    pub fn record_delete(&mut self, silo_id: u64, size: u64) {
        if let Some(quota) = self.quotas.get_mut(&silo_id) {
            quota.used = quota.used.saturating_sub(size);
            quota.object_count = quota.object_count.saturating_sub(1);
            if quota.used <= quota.limit {
                quota.over_quota = false;
                quota.grace_seconds = 0;
            }
            self.total_used = self.total_used.saturating_sub(size);
        }
    }

    /// Increase a silo's quota.
    pub fn increase_quota(&mut self, silo_id: u64, new_limit: u64) {
        if let Some(quota) = self.quotas.get_mut(&silo_id) {
            let old = quota.limit;
            quota.limit = new_limit;
            if quota.used <= new_limit {
                quota.over_quota = false;
            }
            self.log_event(QuotaEvent::Increased { silo_id, old, new: new_limit });
        }
    }

    /// Get usage report for all silos.
    pub fn usage_report(&self) -> Vec<(u64, String, f32, u64, u64)> {
        // Returns: (silo_id, app_name, usage_pct, used, limit)
        self.quotas.values()
            .map(|q| (q.silo_id, q.app_name.clone(), q.usage_percent(), q.used, q.limit))
            .collect()
    }

    /// System-wide usage percentage.
    pub fn system_usage_percent(&self) -> f32 {
        if self.total_capacity == 0 { return 0.0; }
        (self.total_used as f64 / self.total_capacity as f64 * 100.0) as f32
    }

    fn log_event(&mut self, event: QuotaEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
    }
}
