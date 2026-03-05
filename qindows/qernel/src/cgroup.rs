//! # Control Groups — Resource Limits per Silo
//!
//! Enforces hard resource limits on CPU, memory, I/O bandwidth,
//! and GPU per Silo (Section 2.4). Prevents any Silo from
//! monopolizing system resources.
//!
//! Features:
//! - CPU time quota (microseconds per period)
//! - Memory limit (hard cap, soft limit with pressure events)
//! - I/O bandwidth limit (bytes/sec)
//! - GPU compute quota
//! - Hierarchical groups (Silo inherits from parent group)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    CpuTime,
    Memory,
    IoBandwidth,
    GpuCompute,
    NetworkBw,
}

/// Limit enforcement action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Throttle (slow down)
    Throttle,
    /// Kill lowest-priority task
    Kill,
    /// Notify but allow (soft limit)
    Notify,
}

/// A resource limit.
#[derive(Debug, Clone)]
pub struct Limit {
    pub resource: Resource,
    pub hard_limit: u64,
    pub soft_limit: u64,
    pub current: u64,
    pub enforcement: Enforcement,
}

/// A control group.
#[derive(Debug, Clone)]
pub struct CGroup {
    pub id: u64,
    pub name: String,
    pub silo_id: u64,
    pub parent: Option<u64>,
    pub limits: Vec<Limit>,
    pub children: Vec<u64>,
    pub active: bool,
}

/// CGroup statistics.
#[derive(Debug, Clone, Default)]
pub struct CGroupStats {
    pub groups_created: u64,
    pub throttle_events: u64,
    pub kill_events: u64,
    pub soft_limit_events: u64,
    pub hard_limit_events: u64,
}

/// The CGroup Manager.
pub struct CGroupManager {
    pub groups: BTreeMap<u64, CGroup>,
    next_id: u64,
    pub stats: CGroupStats,
}

impl CGroupManager {
    pub fn new() -> Self {
        CGroupManager {
            groups: BTreeMap::new(),
            next_id: 1,
            stats: CGroupStats::default(),
        }
    }

    /// Create a control group for a Silo.
    pub fn create(&mut self, name: &str, silo_id: u64, parent: Option<u64>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        if let Some(pid) = parent {
            if let Some(p) = self.groups.get_mut(&pid) {
                p.children.push(id);
            }
        }

        self.groups.insert(id, CGroup {
            id, name: String::from(name), silo_id,
            parent, limits: Vec::new(), children: Vec::new(), active: true,
        });

        self.stats.groups_created += 1;
        id
    }

    /// Set a resource limit on a group.
    pub fn set_limit(&mut self, group_id: u64, resource: Resource, hard: u64, soft: u64, enforcement: Enforcement) {
        if let Some(group) = self.groups.get_mut(&group_id) {
            // Update existing or add new
            if let Some(limit) = group.limits.iter_mut().find(|l| l.resource == resource) {
                limit.hard_limit = hard;
                limit.soft_limit = soft;
                limit.enforcement = enforcement;
            } else {
                group.limits.push(Limit {
                    resource, hard_limit: hard, soft_limit: soft, current: 0, enforcement,
                });
            }
        }
    }

    /// Charge resource usage.
    pub fn charge(&mut self, group_id: u64, resource: Resource, amount: u64) -> Result<(), Enforcement> {
        let group = match self.groups.get_mut(&group_id) {
            Some(g) => g,
            None => return Ok(()),
        };

        if let Some(limit) = group.limits.iter_mut().find(|l| l.resource == resource) {
            limit.current += amount;

            if limit.current > limit.hard_limit {
                self.stats.hard_limit_events += 1;
                match limit.enforcement {
                    Enforcement::Throttle => self.stats.throttle_events += 1,
                    Enforcement::Kill => self.stats.kill_events += 1,
                    Enforcement::Notify => self.stats.soft_limit_events += 1,
                }
                return Err(limit.enforcement);
            }

            if limit.current > limit.soft_limit {
                self.stats.soft_limit_events += 1;
            }
        }

        // Propagate to parent
        let parent = group.parent;
        if let Some(pid) = parent {
            return self.charge(pid, resource, amount);
        }

        Ok(())
    }

    /// Reset usage counters (called at period boundaries).
    pub fn reset_period(&mut self, group_id: u64, resource: Resource) {
        if let Some(group) = self.groups.get_mut(&group_id) {
            if let Some(limit) = group.limits.iter_mut().find(|l| l.resource == resource) {
                limit.current = 0;
            }
        }
    }

    /// Get effective limit (min of self and ancestors).
    pub fn effective_limit(&self, group_id: u64, resource: Resource) -> u64 {
        let group = match self.groups.get(&group_id) {
            Some(g) => g,
            None => return u64::MAX,
        };

        let self_limit = group.limits.iter()
            .find(|l| l.resource == resource)
            .map(|l| l.hard_limit)
            .unwrap_or(u64::MAX);

        if let Some(pid) = group.parent {
            let parent_limit = self.effective_limit(pid, resource);
            self_limit.min(parent_limit)
        } else {
            self_limit
        }
    }
}
