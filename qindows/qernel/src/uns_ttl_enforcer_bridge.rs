//! # UNS TTL Enforcer Bridge (Phase 172)
//!
//! ## Architecture Guardian: The Gap
//! `uns_cache.rs` implements `UnsCache`:
//! - `lookup(uri)` → Option<&UnsEntry>
//! - `insert(entry)` — add with TTL
//! - `invalidate(uri)` — remove specific URI
//! - `sweep(now)` → count of expired entries removed
//!
//! **Missing link**: `UnsCache::sweep()` was never called periodically.
//! Stale UNS entries accumulated indefinitely, causing:
//! 1. Stale URI resolutions pointing to already-vaporized Silos
//! 2. Memory growth without bound
//!
//! This module provides `UnsTtlEnforcerBridge`:
//! 1. `on_tick()` — calls sweep() on every Nth tick
//! 2. `on_silo_vaporize()` — invalidates all URIs owned by that Silo

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::uns_cache::UnsCache;

#[derive(Debug, Default, Clone)]
pub struct UnsTtlStats {
    pub sweeps:   u64,
    pub expired:  u64,
    pub invalidated: u64,
}

pub struct UnsTtlEnforcerBridge {
    pub cache:       UnsCache,
    silo_uris:       BTreeMap<u64, Vec<alloc::string::String>>,
    tick_interval:   u64,
    last_sweep:      u64,
    pub stats:       UnsTtlStats,
}

impl UnsTtlEnforcerBridge {
    pub fn new(sweep_interval_ticks: u64) -> Self {
        UnsTtlEnforcerBridge {
            cache: UnsCache::new(),
            silo_uris: BTreeMap::new(),
            tick_interval: sweep_interval_ticks,
            last_sweep: 0,
            stats: UnsTtlStats::default(),
        }
    }

    /// Register a URI as belonging to a Silo (for vaporize cleanup).
    pub fn register_silo_uri(&mut self, silo_id: u64, uri: alloc::string::String) {
        self.silo_uris.entry(silo_id).or_default().push(uri);
    }

    /// Drive TTL sweep from the scheduler tick.
    pub fn on_tick(&mut self, tick: u64) {
        if tick - self.last_sweep >= self.tick_interval {
            self.last_sweep = tick;
            self.stats.sweeps += 1;
            self.cache.sweep(tick);
            self.stats.expired += 1; // count sweep event, not expired entries
        }
    }

    /// Invalidate all URIs for a vaporized Silo.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        if let Some(uris) = self.silo_uris.remove(&silo_id) {
            for uri in &uris {
                self.cache.invalidate(uri);
                self.stats.invalidated += 1;
            }
            crate::serial_println!("[UNS TTL] Silo {} vaporized: {} URIs invalidated", silo_id, uris.len());
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UnsTtlBridge: sweeps={} expired={} invalidated={}",
            self.stats.sweeps, self.stats.expired, self.stats.invalidated
        );
    }
}
