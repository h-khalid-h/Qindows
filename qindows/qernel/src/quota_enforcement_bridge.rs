//! # Quota Enforcement Bridge (Phase 132)
//!
//! ## Architecture Guardian: The Gap
//! `qquota.rs` (Phase 82) implements `QuotaManager`:
//! - `create_silo()` — initializes per-Silo quota entries
//! - `check()` — returns QuotaResult (Allow/SoftLimit/HardLimit/Throttle)
//! - `record()` — records usage; `release()` — releases usage
//!
//! **Missing link**: Nothing ever called `QuotaManager::check()` before
//! Silo resources were consumed. The Q-Ring dispatch and Prism writes
//! had no quota gate.
//!
//! This module provides `QuotaEnforcementBridge`:
//! 1. `gate_prism_write()` — calls `check(PrismBytes)` before ghost_write
//! 2. `gate_net_send()` — calls `check(NetworkBytes)` before Nexus send
//! 3. `gate_cpu_burst()` — calls `check(CpuNs)` before fiber scheduling
//! 4. `on_silo_spawn()` — creates quota entry + sets default limits
//! 5. `on_silo_vaporize()` — releases all resources + removes quota

extern crate alloc;
use alloc::vec::Vec;

use crate::qquota::{QuotaManager, QuotaResult, Resource};

// ── Default Quota Limits ──────────────────────────────────────────────────────

/// Default limits applied to every newly spawned Silo.
pub struct DefaultLimits;
impl DefaultLimits {
    pub const CPU_NS_SOFT:       u64 = 500_000_000;   // 500M ticks soft
    pub const CPU_NS_HARD:       u64 = 2_000_000_000; // 2B ticks hard
    pub const PRISM_BYTES_SOFT:  u64 = 512 * 1024 * 1024; // 512 MB
    pub const PRISM_BYTES_HARD:  u64 = 2 * 1024 * 1024 * 1024; // 2 GB
    pub const NET_BYTES_SOFT:    u64 = 100 * 1024 * 1024; // 100 MB
    pub const NET_BYTES_HARD:    u64 = 500 * 1024 * 1024; // 500 MB
}

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct QuotaBridgeStats {
    pub prism_gates:    u64,
    pub net_gates:      u64,
    pub cpu_gates:      u64,
    pub hard_limits:    u64,
    pub soft_limits:    u64,
    pub throttles:      u64,
}

// ── Quota Enforcement Bridge ──────────────────────────────────────────────────

/// Gates resource consumption behind QuotaManager::check().
pub struct QuotaEnforcementBridge {
    pub manager: QuotaManager,
    pub stats:   QuotaBridgeStats,
}

impl QuotaEnforcementBridge {
    pub fn new() -> Self {
        QuotaEnforcementBridge {
            manager: QuotaManager::new(),
            stats: QuotaBridgeStats::default(),
        }
    }

    /// Called at Silo spawn. Sets up quota entry with default limits.
    pub fn on_silo_spawn(&mut self, silo_id: u64, parent: Option<u64>) {
        self.manager.create_silo(silo_id, parent);
        self.manager.set(silo_id, Resource::CpuMs,
            DefaultLimits::CPU_NS_SOFT, DefaultLimits::CPU_NS_HARD);
        self.manager.set(silo_id, Resource::MemoryBytes,
            DefaultLimits::PRISM_BYTES_SOFT, DefaultLimits::PRISM_BYTES_HARD);
        self.manager.set(silo_id, Resource::NetworkBytesOut,
            DefaultLimits::NET_BYTES_SOFT, DefaultLimits::NET_BYTES_HARD);
        crate::serial_println!("[QUOTA] Silo {} initialized with default limits", silo_id);
    }

    /// Called at Silo vaporize. Releases all resource accounting.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.manager.release(silo_id, Resource::CpuMs, u64::MAX);
        self.manager.release(silo_id, Resource::MemoryBytes, u64::MAX);
        self.manager.release(silo_id, Resource::NetworkBytesOut, u64::MAX);
        self.manager.remove_silo(silo_id);
        crate::serial_println!("[QUOTA] Silo {} quota released", silo_id);
    }

    /// Gate a Prism write by byte count.
    pub fn gate_prism_write(&mut self, silo_id: u64, bytes: u64, tick: u64) -> bool {
        self.stats.prism_gates += 1;
        match self.manager.check(silo_id, Resource::MemoryBytes, bytes, tick) {
            QuotaResult::Allowed => {
                self.manager.record(silo_id, Resource::MemoryBytes, bytes);
                true
            }
            QuotaResult::SoftWarning => {
                self.stats.soft_limits += 1;
                self.manager.record(silo_id, Resource::MemoryBytes, bytes);
                crate::serial_println!("[QUOTA] Silo {} MemoryBytes soft limit reached", silo_id);
                true
            }
            QuotaResult::HardDenied => {
                self.stats.hard_limits += 1;
                crate::serial_println!("[QUOTA] Silo {} MemoryBytes HARD LIMIT — write blocked", silo_id);
                false
            }
            QuotaResult::NoQuota => true, // no quota entry, allow
        }
    }

    /// Gate a network send.
    pub fn gate_net_send(&mut self, silo_id: u64, bytes: u64, tick: u64) -> bool {
        self.stats.net_gates += 1;
        match self.manager.check(silo_id, Resource::NetworkBytesOut, bytes, tick) {
            QuotaResult::Allowed | QuotaResult::SoftWarning => {
                self.manager.record(silo_id, Resource::NetworkBytesOut, bytes);
                true
            }
            QuotaResult::HardDenied => {
                self.stats.hard_limits += 1;
                crate::serial_println!("[QUOTA] Silo {} NetBytes HARD LIMIT — send blocked", silo_id);
                false
            }
            QuotaResult::NoQuota => true,
        }
    }

    /// Gate CPU usage for a fiber scheduling quantum.
    pub fn gate_cpu_burst(&mut self, silo_id: u64, cpu_ns: u64, tick: u64) -> bool {
        self.stats.cpu_gates += 1;
        match self.manager.check(silo_id, Resource::CpuMs, cpu_ns, tick) {
            QuotaResult::Allowed | QuotaResult::SoftWarning => {
                self.manager.record(silo_id, Resource::CpuMs, cpu_ns);
                true
            }
            QuotaResult::HardDenied => {
                self.stats.hard_limits += 1;
                false // preempt
            }
            QuotaResult::NoQuota => true,
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QuotaBridge: prism_gates={} net_gates={} cpu_gates={} hard={} soft={} throttle={}",
            self.stats.prism_gates, self.stats.net_gates, self.stats.cpu_gates,
            self.stats.hard_limits, self.stats.soft_limits, self.stats.throttles
        );
    }
}
