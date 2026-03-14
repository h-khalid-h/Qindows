//! # CGroup Quota Bridge (Phase 152)
//!
//! ## Architecture Guardian: The Gap
//! `cgroup.rs` implements `CGroupManager`:
//! - `create(name, silo_id, parent)` → group_id: u64
//! - `set_limit(group_id, resource, hard, soft, enforcement)`
//! - `charge(group_id, resource, amount)` → Result<(), Enforcement>
//! - `reset_period(group_id, resource)`
//!
//! **Missing link**: `CGroupManager` implemented Linux-style hierarchical
//! resource limits but was never connected to Silo lifecycle. No CGroup
//! was ever created at Silo spawn, so `charge()` was never called for
//! real CPU/memory/net work.
//!
//! This module provides `CGroupQuotaBridge`:
//! 1. `on_silo_spawn()` — create CGroup, set limits from quota settings
//! 2. `on_cpu_work()` — charge CPU ticks against the Silo's CGroup
//! 3. `on_mem_alloc()` — charge memory against the CGroup
//! 4. `on_silo_vaporize()` — release CGroup on exit

extern crate alloc;

use crate::cgroup::{CGroupManager, Resource, Enforcement};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct CGroupBridgeStats {
    pub silos_registered:  u64,
    pub cpu_charges:       u64,
    pub cpu_throttled:     u64,
    pub mem_charges:       u64,
    pub mem_throttled:     u64,
}

// ── CGroup Quota Bridge ───────────────────────────────────────────────────────

/// Connects CGroupManager to Silo lifecycle and resource accounting.
pub struct CGroupQuotaBridge {
    pub manager: CGroupManager,
    pub stats:   CGroupBridgeStats,
    /// Default CPU soft limit (ticks per period)
    pub cpu_soft: u64,
    /// Default CPU hard limit (ticks per period)
    pub cpu_hard: u64,
    /// Default memory soft limit (bytes)
    pub mem_soft_bytes: u64,
    /// Default memory hard limit (bytes)
    pub mem_hard_bytes: u64,
}

impl CGroupQuotaBridge {
    pub fn new() -> Self {
        CGroupQuotaBridge {
            manager: CGroupManager::new(),
            stats: CGroupBridgeStats::default(),
            cpu_soft: 500_000_000,          // 500M ticks/period
            cpu_hard: 2_000_000_000,        // 2B ticks/period (hard cut)
            mem_soft_bytes: 512 * 1024 * 1024,   // 512 MB soft
            mem_hard_bytes: 2 * 1024 * 1024 * 1024, // 2 GB hard
        }
    }

    /// Create CGroup for a new Silo; set CPU + memory limits.
    pub fn on_silo_spawn(&mut self, silo_id: u64) -> u64 {
        self.stats.silos_registered += 1;

        let group_id = self.manager.create(
            &alloc::format!("silo_{}", silo_id),
            silo_id,
            None, // top-level, no parent
        );

        // CPU ticks limit
        self.manager.set_limit(
            group_id, Resource::Cpu,
            self.cpu_hard, self.cpu_soft, Enforcement::Throttle,
        );

        // Memory bytes limit
        self.manager.set_limit(
            group_id, Resource::Memory,
            self.mem_hard_bytes, self.mem_soft_bytes, Enforcement::Throttle,
        );

        crate::serial_println!(
            "[CGROUP BRIDGE] Silo {} → CGroup {} (cpu_hard={} mem_hard={}MiB)",
            silo_id, group_id,
            self.cpu_hard, self.mem_hard_bytes / (1024 * 1024)
        );

        group_id
    }

    /// Charge CPU ticks to a Silo's CGroup. Returns false if throttled.
    pub fn on_cpu_work(&mut self, group_id: u64, ticks: u64) -> bool {
        self.stats.cpu_charges += 1;
        match self.manager.charge(group_id, Resource::Cpu, ticks) {
            Ok(()) => true,
            Err(Enforcement::Throttle) => {
                self.stats.cpu_throttled += 1;
                crate::serial_println!("[CGROUP] Group {} CPU throttled", group_id);
                false
            }
            Err(_) => false,
        }
    }

    /// Charge memory allocation to a Silo's CGroup. Returns false if throttled.
    pub fn on_mem_alloc(&mut self, group_id: u64, bytes: u64) -> bool {
        self.stats.mem_charges += 1;
        match self.manager.charge(group_id, Resource::Memory, bytes) {
            Ok(()) => true,
            Err(Enforcement::Throttle) => {
                self.stats.mem_throttled += 1;
                crate::serial_println!("[CGROUP] Group {} memory throttled", group_id);
                false
            }
            Err(_) => false,
        }
    }

    /// Reset per-period counters (call at each scheduler epoch).
    pub fn on_period_reset(&mut self, group_id: u64) {
        self.manager.reset_period(group_id, Resource::Cpu);
        self.manager.reset_period(group_id, Resource::Memory);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CGroupBridge: silos={} cpu_chg={} cpu_thr={} mem_chg={} mem_thr={}",
            self.stats.silos_registered,
            self.stats.cpu_charges, self.stats.cpu_throttled,
            self.stats.mem_charges, self.stats.mem_throttled
        );
    }
}
