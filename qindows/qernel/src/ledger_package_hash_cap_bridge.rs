//! # Ledger Package Hash Cap Bridge (Phase 273)
//!
//! ## Architecture Guardian: The Gap
//! `ledger.rs` implements the Q-Ledger app package registry:
//! - `AppManifest { hash: PackageHash, capabilities: ManifestCapability, ... }`
//! - `ManifestCapability` — enum of capability requirements
//! - `LedgerError` — HashMismatch, UnknownPackage, ...
//!
//! **Missing link**: Package publishing rate was uncapped. A publisher
//! could flood the ledger with thousands of package versions per tick,
//! bloating the packages registry and evicting older trusted packages.
//!
//! This module provides `LedgerPackageHashCapBridge`:
//! Max 4 package publishes per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_PUBLISHES_PER_SILO_PER_TICK: u64 = 4;

#[derive(Debug, Default, Clone)]
pub struct LedgerPublishCapStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct LedgerPackageHashCapBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    LedgerPublishCapStats,
}

impl LedgerPackageHashCapBridge {
    pub fn new() -> Self {
        LedgerPackageHashCapBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: LedgerPublishCapStats::default() }
    }

    pub fn allow_publish(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_PUBLISHES_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            crate::serial_println!(
                "[LEDGER] Silo {} package publish rate limit reached ({}/{})", silo_id, count, MAX_PUBLISHES_PER_SILO_PER_TICK
            );
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  LedgerPublishCapBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
