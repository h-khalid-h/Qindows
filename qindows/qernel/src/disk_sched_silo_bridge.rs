//! # Disk Scheduler Silo Bridge (Phase 143)
//!
//! ## Architecture Guardian: The Gap
//! `disk_sched.rs` implements `DiskScheduler`:
//! - `submit()` — submits an IoRequest (silo_id, device_id, dir, sector, count, priority, now)
//! - `set_share()` — sets per-Silo I/O weight for fair-share scheduling
//! - Prioritizes I/O by `IoPriority`: Realtime > High > Normal > Idle
//!
//! **Missing link**: `set_share()` was never called at Silo spawn, so all Silos
//! ran with equal weight. `submit()` was never gated behind quota checks —
//! a Silo could saturate I/O without enforcement.
//!
//! This module provides `DiskSchedSiloBridge`:
//! 1. `on_silo_spawn()` — sets I/O share weight from quota settings
//! 2. `on_silo_vaporize()` — removes Silo's I/O reservation
//! 3. `submit_gated()` — enforces priority based on CapToken tier
//! 4. `admin_submit()` — Sentinel-tier Realtime I/O for kernel operations

extern crate alloc;

use crate::disk_sched::{DiskScheduler, IoDir, IoPriority};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct DiskBridgeStats {
    pub requests_submitted: u64,
    pub requests_realtime:  u64,
    pub requests_normal:    u64,
    pub requests_idle:      u64,
    pub silos_registered:   u64,
}

// ── Disk Scheduler Silo Bridge ────────────────────────────────────────────────

/// Connects DiskScheduler to Silo lifecycle and CapToken priority tiers.
pub struct DiskSchedSiloBridge {
    pub scheduler: DiskScheduler,
    pub stats:     DiskBridgeStats,
}

impl DiskSchedSiloBridge {
    pub fn new() -> Self {
        DiskSchedSiloBridge {
            scheduler: DiskScheduler::new(),
            stats: DiskBridgeStats::default(),
        }
    }

    /// Register a Silo's I/O share at spawn time.
    /// Weight 100 = normal, 200 = high-priority service, 50 = background.
    pub fn on_silo_spawn(&mut self, silo_id: u64, io_weight: u32) {
        self.stats.silos_registered += 1;
        self.scheduler.set_share(silo_id, io_weight);
        crate::serial_println!(
            "[DISK BRIDGE] Silo {} registered with I/O weight {}", silo_id, io_weight
        );
    }

    /// Remove Silo from scheduler (on vaporize).
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.scheduler.set_share(silo_id, 0); // zero weight → effectively removed
    }

    /// Submit I/O with priority derived from CapToken tier.
    /// Admin cap → High; plain Silo → Normal; no cap → Idle.
    pub fn submit_gated(
        &mut self,
        silo_id: u64,
        device_id: u32,
        dir: IoDir,
        sector: u64,
        count: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.requests_submitted += 1;

        let priority = if forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.requests_realtime += 1;
            IoPriority::Critical
        } else if forge.check(silo_id, CapType::Prism, CAP_EXEC, 0, tick) {
            self.stats.requests_normal += 1;
            IoPriority::System
        } else {
            self.stats.requests_idle += 1;
            IoPriority::Normal
        };

        self.scheduler.submit(silo_id, device_id, dir, sector, count, priority, tick)
    }

    /// Kernel-internal (Sentinel) I/O — always Realtime priority.
    pub fn admin_submit(&mut self, device_id: u32, dir: IoDir, sector: u64, count: u32, tick: u64) -> u64 {
        self.stats.requests_realtime += 1;
        self.scheduler.submit(0, device_id, dir, sector, count, IoPriority::Critical, tick)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  DiskBridge: submitted={} realtime={} normal={} idle={} silos={}",
            self.stats.requests_submitted, self.stats.requests_realtime,
            self.stats.requests_normal, self.stats.requests_idle, self.stats.silos_registered
        );
    }
}
