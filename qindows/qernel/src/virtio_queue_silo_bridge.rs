//! # VirtIO Queue Silo Bridge (Phase 253)
//!
//! ## Architecture Guardian: The Gap
//! `virtio.rs` implements `VirtQueue`:
//! - `alloc_desc(count: u16)` → Option<u16> — allocate descriptor chain
//! - `free_desc(head, count)` — free descriptor chain
//!
//! **Missing link**: `alloc_desc()` had no per-Silo cap on descriptor
//! allocation. A Silo could exhaust the entire descriptor ring (typically
//! 256 entries), starving all other VirtIO device communications.
//!
//! This module provides `VirtioQueueSiloBridge`:
//! Max 32 descriptors per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_DESC_PER_SILO_PER_TICK: u64 = 32;

#[derive(Debug, Default, Clone)]
pub struct VirtioQueueSiloStats {
    pub allocs_allowed: u64,
    pub allocs_denied:  u64,
}

pub struct VirtioQueueSiloBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    VirtioQueueSiloStats,
}

impl VirtioQueueSiloBridge {
    pub fn new() -> Self {
        VirtioQueueSiloBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: VirtioQueueSiloStats::default() }
    }

    pub fn allow_alloc(&mut self, silo_id: u64, count: u16, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let used = self.tick_counts.entry(silo_id).or_default();
        if *used + count as u64 > MAX_DESC_PER_SILO_PER_TICK {
            self.stats.allocs_denied += 1;
            crate::serial_println!(
                "[VIRTIO] Silo {} desc alloc {} denied — quota {}/{}", silo_id, count, used, MAX_DESC_PER_SILO_PER_TICK
            );
            return false;
        }
        *used += count as u64;
        self.stats.allocs_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  VirtioQueueBridge: allowed={} denied={}",
            self.stats.allocs_allowed, self.stats.allocs_denied
        );
    }
}
