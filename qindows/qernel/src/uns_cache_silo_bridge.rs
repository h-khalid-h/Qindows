//! # UNS Cache Silo Invalidation Bridge (Phase 145)
//!
//! ## Architecture Guardian: The Gap
//! `uns_cache.rs` implements `UnsCache`:
//! - `lookup()` — resolve a URI from L1/L2 cache
//! - `insert()` — cache a resolved address
//! - `insert_negative()` — cache a failed resolution
//! - `invalidate()` — remove a specific URI from the cache
//! - `sweep()` — TTL expiry sweep
//!
//! **Missing link**: When a Silo vaporizes, any UNS URIs it owned
//! remained in cache and continued resolving to dead addresses.
//! Subsequent Nexus routing queries got stale results.
//!
//! This module provides `UnsCacheSiloBridge`:
//! 1. `on_silo_vaporize()` — invalidates all URIs tied to that Silo
//! 2. `resolve_or_cache()` — wraps lookup + auto-negative on miss
//! 3. `on_tick()` — drives TTL sweep

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::uns_cache::{UnsCache, ResolvedAddr};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct UnsBridgeStats {
    pub entries_invalidated: u64,
    pub silos_cleared:       u64,
    pub cache_hits:          u64,
    pub cache_misses:        u64,
    pub sweeps:              u64,
}

// ── UNS Cache Silo Bridge ─────────────────────────────────────────────────────

/// Keeps UnsCache consistent with Silo lifecycle events.
pub struct UnsCacheSiloBridge {
    pub cache: UnsCache,
    pub stats: UnsBridgeStats,
    /// Tracks (silo_id, uri) pairs registered at spawn time
    silo_uris: alloc::collections::BTreeMap<u64, Vec<String>>,
}

impl UnsCacheSiloBridge {
    pub fn new() -> Self {
        UnsCacheSiloBridge {
            cache: UnsCache::new(),
            stats: UnsBridgeStats::default(),
            silo_uris: alloc::collections::BTreeMap::new(),
        }
    }

    /// Register a URI as belonging to a specific Silo.
    /// Called when a Silo publishes a service to the UNS.
    pub fn register_silo_uri(&mut self, silo_id: u64, uri: String, addr: ResolvedAddr, tick: u64) {
        self.cache.insert(&uri, addr, tick);
        self.silo_uris.entry(silo_id).or_default().push(uri);
    }

    /// Purge all cached UNS entries that belong to a vaporized Silo.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.stats.silos_cleared += 1;

        let uris = self.silo_uris.remove(&silo_id).unwrap_or_default();
        let count = uris.len() as u64;

        for uri in &uris {
            self.cache.invalidate(uri.as_str());
        }

        self.stats.entries_invalidated += count;

        if count > 0 {
            crate::serial_println!(
                "[UNS BRIDGE] Silo {} vaporized: {} UNS entries invalidated", silo_id, count
            );
        }
    }

    /// Look up a URI; return cached result or None.
    pub fn resolve(&mut self, uri: &str, tick: u64) -> Option<ResolvedAddr> {
        if let Some(addr) = self.cache.lookup(uri, tick) {
            self.stats.cache_hits += 1;
            return Some(addr);
        }
        self.stats.cache_misses += 1;
        None
    }

    /// Drive TTL sweep (call every N ticks from APIC timer).
    pub fn on_tick(&mut self, tick: u64) {
        self.stats.sweeps += 1;
        self.cache.sweep(tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UnsBridge: invalidated={} silos_cleared={} hits={} misses={} sweeps={}",
            self.stats.entries_invalidated, self.stats.silos_cleared,
            self.stats.cache_hits, self.stats.cache_misses, self.stats.sweeps
        );
    }
}
