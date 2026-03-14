//! # UNS Resolution Rate Bridge (Phase 236)
//!
//! ## Architecture Guardian: The Gap
//! `uns.rs` implements the Universal Naming System:
//! - `UnsUri::parse(uri)` → Option<Self>
//! - `UnsTarget` — Silo, Resource, Mesh node, Service
//! - `UnsResolution` — resolved address
//! - `UnsMountPoint` — namespace mount points
//!
//! **Missing link**: UNS resolution had no per-Silo rate limit.
//! A Silo could flood the UNS with resolution requests, starving
//! DNS-like lookups for other Silos (Law 4 DoS).
//!
//! This module provides `UnsResolutionRateBridge`:
//! Max 64 UNS resolutions per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_UNS_RESOLUTIONS_PER_SILO_PER_TICK: u64 = 64;

#[derive(Debug, Default, Clone)]
pub struct UnsRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct UnsResolutionRateBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    UnsRateStats,
}

impl UnsResolutionRateBridge {
    pub fn new() -> Self {
        UnsResolutionRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: UnsRateStats::default() }
    }

    pub fn allow_resolve(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_UNS_RESOLUTIONS_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UnsRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
