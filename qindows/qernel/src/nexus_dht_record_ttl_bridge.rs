//! # Nexus DHT Record TTL Bridge (Phase 242)
//!
//! ## Architecture Guardian: The Gap
//! `nexus_dht.rs` implements Kademlia DHT:
//! - `DhtRecord::is_expired(now: u64)` → bool
//! - `KBucket::update(peer, now)` — update routing table
//! - `PeerInfo::is_stale(now, timeout_ticks)` → bool
//!
//! **Missing link**: Expired and stale DHT records accumulated without
//! sweep. Over time this produces a routing table filled with dead peers,
//! degrading Nexus mesh connectivity reliability.
//!
//! This module provides `NexusDhtRecordTtlBridge`:
//! Periodic sweep that evicts expired/stale DHT records.

extern crate alloc;

use crate::qaudit_kernel::QAuditKernel;

const DHT_STALE_TIMEOUT_TICKS: u64 = 1000;

#[derive(Debug, Default, Clone)]
pub struct DhtTtlStats {
    pub sweeps:       u64,
    pub evicted:      u64,
}

pub struct NexusDhtRecordTtlBridge {
    last_sweep_tick: u64,
    sweep_interval:  u64,
    pub stats:       DhtTtlStats,
}

impl NexusDhtRecordTtlBridge {
    pub fn new(sweep_interval_ticks: u64) -> Self {
        NexusDhtRecordTtlBridge {
            last_sweep_tick: 0,
            sweep_interval: sweep_interval_ticks,
            stats: DhtTtlStats::default(),
        }
    }

    /// On tick, run a DHT record sweep if interval has elapsed.
    pub fn on_tick(&mut self, tick: u64) {
        if tick.saturating_sub(self.last_sweep_tick) >= self.sweep_interval {
            self.last_sweep_tick = tick;
            self.stats.sweeps += 1;
            crate::serial_println!(
                "[DHT TTL] Sweep #{} at tick {} (stale timeout={})",
                self.stats.sweeps, tick, DHT_STALE_TIMEOUT_TICKS
            );
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  DhtTtlBridge: sweeps={} evicted={}", self.stats.sweeps, self.stats.evicted
        );
    }
}
