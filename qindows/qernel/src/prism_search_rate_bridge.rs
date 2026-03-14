//! # Prism Search Rate Bridge (Phase 260)
//!
//! ## Architecture Guardian: The Gap
//! `prism_search.rs` implements graph-based semantic search:
//! - Full object graph traversal for intent resolution
//!
//! **Missing link**: Semantic search calls were unbounded. A Silo could
//! fire hundreds of complex graph queries per tick, triggering graph lock
//! contention and degrading search latency for all Silos.
//!
//! This module provides `PrismSearchRateBridge`:
//! Max 16 search queries per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_SEARCHES_PER_SILO_PER_TICK: u64 = 16;

#[derive(Debug, Default, Clone)]
pub struct PrismSearchRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct PrismSearchRateBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    PrismSearchRateStats,
}

impl PrismSearchRateBridge {
    pub fn new() -> Self {
        PrismSearchRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: PrismSearchRateStats::default() }
    }

    pub fn allow_search(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_SEARCHES_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PrismSearchRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
