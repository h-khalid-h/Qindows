//! # CGroup Hard Limit Bridge (Phase 222)
//!
//! ## Architecture Guardian: The Gap
//! `cgroup.rs` implements `CGroupManager`:
//! - `create(name, silo_id, parent)` → cgroup_id
//! - `Resource` enum { Cpu, Memory, Io, Network, Storage }
//! - `Limit { soft: u64, hard: u64, enforcement: Enforcement }`
//! - `Enforcement { SoftWarning, HardKill, HardThrottle }`
//!
//! **Missing link**: CGroup limits could be set to soft-only for all
//! resource types. A Silo could set soft limits only, effectively bypassing
//! any enforcement — all limits were advisory, never hard.
//!
//! This module provides `CGroupHardLimitBridge`:
//! Enforces that all Silo cgroups use HardThrottle or HardKill enforcement.

extern crate alloc;

use crate::cgroup::{CGroupManager, Resource, Enforcement, Limit};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct CGroupLimitStats {
    pub limits_set:        u64,
    pub soft_only_blocked: u64,
}

pub struct CGroupHardLimitBridge {
    pub manager: CGroupManager,
    pub stats:   CGroupLimitStats,
}

impl CGroupHardLimitBridge {
    pub fn new() -> Self {
        CGroupHardLimitBridge { manager: CGroupManager::new(), stats: CGroupLimitStats::default() }
    }

    /// Create a cgroup — requires Admin:EXEC cap.
    pub fn create(
        &mut self,
        name: &str,
        silo_id: u64,
        parent: Option<u64>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            return None;
        }
        Some(self.manager.create(name, silo_id, parent))
    }

    /// Validate that a proposed limit uses hard enforcement.
    /// Returns the limit with hard enforcement if soft-only was attempted.
    pub fn enforce_hard_limit(&mut self, proposed: Limit) -> Limit {
        self.stats.limits_set += 1;
        match proposed.enforcement {
            Enforcement::Notify => {
                self.stats.soft_only_blocked += 1;
                crate::serial_println!("[CGROUP] Notify-only limit blocked — upgrading to Throttle");
                Limit { enforcement: Enforcement::Throttle, ..proposed }
            }
            _ => proposed,
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CGroupBridge: limits={} soft_blocked={}",
            self.stats.limits_set, self.stats.soft_only_blocked
        );
    }
}
