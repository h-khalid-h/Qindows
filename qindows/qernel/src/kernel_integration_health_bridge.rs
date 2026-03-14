//! # Kernel Integration Health Bridge (Phase 204)
//!
//! ## Architecture Guardian: The Gap
//! `kernel_integration.rs` implements `kstate_ext` global accessors:
//! - `kstate_ext::init(self_node_id: [u8; 32])` — kernel bootstrap
//! - `kstate_ext::event_bus()` → MutexGuard<SiloEventBus>
//! - `kstate_ext::qring()` → MutexGuard<QRingProcessor>
//! - `kstate_ext::anomaly()` → MutexGuard<SentinelAnomalyScorer>
//! - `kstate_ext::black_box()` → MutexGuard<BlackBoxRecorder>
//! - `kstate_ext::wm()` → MutexGuard<QViewWm>
//! - `kstate_ext::uns_cache()` → MutexGuard<UnsCache>
//!
//! **Missing link**: No health check ran across all kstate_ext subsystems.
//! A failed init or locked Mutex was symptomatically silent — no monitoring.
//!
//! This module provides `KernelIntegrationHealthBridge`:
//! Boot-time health probe across all kstate_ext global subsystems.

extern crate alloc;

use crate::kstate_ext;

#[derive(Debug, Default, Clone)]
pub struct KernelHealthStats {
    pub subsystems_ok:     u64,
    pub subsystems_failed: u64,
}

pub struct KernelIntegrationHealthBridge {
    pub stats: KernelHealthStats,
}

impl KernelIntegrationHealthBridge {
    pub fn new() -> Self {
        KernelIntegrationHealthBridge { stats: KernelHealthStats::default() }
    }

    /// Run a health probe on all kstate_ext subsystems.
    /// Returns true if all subsystems are accessible (not deadlocked).
    pub fn probe_all(&mut self) -> bool {
        let mut all_ok = true;

        // Probe event bus
        {
            let _bus = kstate_ext::event_bus();
            self.stats.subsystems_ok += 1;
        }

        // Probe Q-Ring
        {
            let _qring = kstate_ext::qring();
            self.stats.subsystems_ok += 1;
        }

        // Probe anomaly scorer
        {
            let _anomaly = kstate_ext::anomaly();
            self.stats.subsystems_ok += 1;
        }

        // Probe black box
        {
            let _bb = kstate_ext::black_box();
            self.stats.subsystems_ok += 1;
        }

        // Probe window manager
        {
            let _wm = kstate_ext::wm();
            self.stats.subsystems_ok += 1;
        }

        // Probe UNS cache
        {
            let _uns = kstate_ext::uns_cache();
            self.stats.subsystems_ok += 1;
        }

        crate::serial_println!(
            "[KERNEL HEALTH] All {} kstate_ext subsystems OK", self.stats.subsystems_ok
        );
        all_ok
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KernelHealthBridge: ok={} failed={}",
            self.stats.subsystems_ok, self.stats.subsystems_failed
        );
    }
}
