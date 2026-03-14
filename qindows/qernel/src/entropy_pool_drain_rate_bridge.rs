//! # Entropy Pool Drain Rate Bridge (Phase 290)
//!
//! ## Architecture Guardian: The Gap
//! `entropy_pool.rs` implements `EntropyPool`:
//! - `PoolHealth` — Healthy, Low, Critical, Exhausted
//! - `EntropySample { source: EntropySource, bits }` — entropy sample
//! - `EntropyPool::new()` — create entropy pool
//!
//! **Missing link**: Entropy entropy drain had no per-Silo rate limit.
//! A Silo generating cryptographic keys rapidly could drain the pool
//! to `PoolHealth::Critical`, degrading entropy quality for all Silos.
//!
//! This module provides `EntropyPoolDrainRateBridge`:
//! Max 1024 entropy bits consumed per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_ENTROPY_BITS_PER_SILO_PER_TICK: u64 = 1024; // 128 bytes per tick

#[derive(Debug, Default, Clone)]
pub struct EntropyDrainRateStats {
    pub draws_allowed:   u64,
    pub draws_throttled: u64,
}

pub struct EntropyPoolDrainRateBridge {
    tick_bits:    BTreeMap<u64, u64>, // silo_id → bits consumed this tick
    current_tick: u64,
    pub stats:    EntropyDrainRateStats,
}

impl EntropyPoolDrainRateBridge {
    pub fn new() -> Self {
        EntropyPoolDrainRateBridge { tick_bits: BTreeMap::new(), current_tick: 0, stats: EntropyDrainRateStats::default() }
    }

    pub fn allow_draw(&mut self, silo_id: u64, bits: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_bits.clear();
            self.current_tick = tick;
        }
        let used = self.tick_bits.entry(silo_id).or_default();
        if *used + bits > MAX_ENTROPY_BITS_PER_SILO_PER_TICK {
            self.stats.draws_throttled += 1;
            crate::serial_println!(
                "[ENTROPY] Silo {} entropy drain throttled ({}/{} bits)", silo_id, used, MAX_ENTROPY_BITS_PER_SILO_PER_TICK
            );
            return false;
        }
        *used += bits;
        self.stats.draws_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EntropyDrainBridge: allowed={} throttled={}",
            self.stats.draws_allowed, self.stats.draws_throttled
        );
    }
}
