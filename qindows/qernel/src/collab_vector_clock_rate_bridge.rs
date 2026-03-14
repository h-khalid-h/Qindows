//! # Collab Vector Clock Rate Bridge (Phase 266)
//!
//! ## Architecture Guardian: The Gap
//! `collab.rs` implements `VectorClock`:
//! - `VectorClock::tick(node: NodeId)` — advance a node's clock
//! - `VectorClock::merge(other)` — merge another clock into self
//! - `VectorClock::happens_before(other)` → bool
//!
//! **Missing link**: `VectorClock::tick()` was called without rate
//! limiting. A Silo could fire thousands of vector clock ticks per
//! quantum, causing unbounded clock drift and invalidating distributed
//! ordering guarantees across the mesh.
//!
//! This module provides `CollabVectorClockRateBridge`:
//! Max 64 clock ticks per node per tick (prevents clock inflation).

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_CLOCK_TICKS_PER_NODE_PER_TICK: u64 = 64;

#[derive(Debug, Default, Clone)]
pub struct VectorClockRateStats {
    pub ticks_allowed: u64,
    pub ticks_denied:  u64,
}

pub struct CollabVectorClockRateBridge {
    tick_counts:  BTreeMap<u64, u64>, // silo_id → count
    current_tick: u64,
    pub stats:    VectorClockRateStats,
}

impl CollabVectorClockRateBridge {
    pub fn new() -> Self {
        CollabVectorClockRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: VectorClockRateStats::default() }
    }

    pub fn allow_tick(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_CLOCK_TICKS_PER_NODE_PER_TICK {
            self.stats.ticks_denied += 1;
            return false;
        }
        *count += 1;
        self.stats.ticks_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  VectorClockRateBridge: allowed={} denied={}",
            self.stats.ticks_allowed, self.stats.ticks_denied
        );
    }
}
