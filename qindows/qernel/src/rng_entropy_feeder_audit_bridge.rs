//! # RNG Entropy Feeder Audit Bridge (Phase 201)
//!
//! ## Architecture Guardian: The Gap
//! `rng_entropy_feeder.rs` implements `RngEntropyFeeder`:
//! - `feed_timer_entropy(tsc, tick)` — feed TSC-based entropy
//! - `feed_pmc_entropy(branch_misses, tick)` — feed PMC entropy
//! - `check_refresh(tick)` — reseed if below threshold
//! - `generate(n: usize)` → Vec<u8> — generate n random bytes
//!
//! **Missing link**: `generate()` could be called by any code path without
//! checking that the feeder had recently been refreshed. Stale/low-entropy
//! pools could generate weak random numbers for cryptographic operations.
//!
//! This module provides `RngEntropyFeederAuditBridge`:
//! Calls `check_refresh(tick)` before every `generate()` to ensure freshness.

extern crate alloc;
use alloc::vec::Vec;

use crate::rng_entropy_feeder::RngEntropyFeeder;

#[derive(Debug, Default, Clone)]
pub struct EntropyFeederBridgeStats {
    pub refreshes:    u64,
    pub generates:    u64,
}

pub struct RngEntropyFeederAuditBridge {
    pub feeder: RngEntropyFeeder,
    pub stats:  EntropyFeederBridgeStats,
}

impl RngEntropyFeederAuditBridge {
    pub fn new() -> Self {
        RngEntropyFeederAuditBridge { feeder: RngEntropyFeeder::new(), stats: EntropyFeederBridgeStats::default() }
    }

    /// Generate n random bytes — always calls check_refresh first.
    pub fn generate_with_refresh(&mut self, n: usize, tick: u64) -> Vec<u8> {
        self.feeder.check_refresh(tick);
        self.stats.refreshes += 1;
        self.stats.generates += 1;
        self.feeder.generate(n)
    }

    /// Feed entropy from hardware sources (call on every tick from timer ISR).
    pub fn feed_timer(&mut self, tsc: u64, tick: u64) {
        self.feeder.feed_timer_entropy(tsc, tick);
    }

    pub fn feed_pmc(&mut self, branch_misses: u64, tick: u64) {
        self.feeder.feed_pmc_entropy(branch_misses, tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EntropyFeederBridge: refreshes={} generates={}",
            self.stats.refreshes, self.stats.generates
        );
    }
}
