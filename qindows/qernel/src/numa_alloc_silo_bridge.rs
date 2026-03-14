//! # NUMA Allocator Silo Bridge (Phase 244)
//!
//! ## Architecture Guardian: The Gap
//! `numa_alloc.rs` implements `NumaAllocator`:
//! - `set_silo_affinity(silo_id, node_id)` — pin Silo allocations to NUMA node
//! - `alloc_page(silo_id)` → Option<u64> — allocate page from Silo's NUMA node
//! - `free_page(pfn)` — free a physical page frame
//!
//! **Missing link**: `set_silo_affinity()` could pin all Silos to the
//! same NUMA node, creating extreme memory imbalance. Node 0 exhausted
//! while Node 1 sits idle — unnecessary cross-node latency.
//!
//! This module provides `NumaAllocatorSiloBridge`:
//! Tracks per-node Silo distribution and warns on imbalance.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::numa_alloc::NumaAllocator;

#[derive(Debug, Default, Clone)]
pub struct NumaAllocStats {
    pub affinity_sets: u64,
    pub imbalance_warnings: u64,
}

pub struct NumaAllocatorSiloBridge {
    pub allocator:  NumaAllocator,
    node_silo_count: BTreeMap<u32, u32>,
    pub stats:      NumaAllocStats,
}

const MAX_SILOS_PER_NODE_BEFORE_WARN: u32 = 32;

impl NumaAllocatorSiloBridge {
    pub fn new() -> Self {
        NumaAllocatorSiloBridge { allocator: NumaAllocator::new(), node_silo_count: BTreeMap::new(), stats: NumaAllocStats::default() }
    }

    pub fn set_silo_affinity(&mut self, silo_id: u64, node_id: u32) {
        self.stats.affinity_sets += 1;
        let count = self.node_silo_count.entry(node_id).or_default();
        *count += 1;
        if *count > MAX_SILOS_PER_NODE_BEFORE_WARN {
            self.stats.imbalance_warnings += 1;
            crate::serial_println!(
                "[NUMA ALLOC] Node {} has {} Silos — imbalance risk", node_id, count
            );
        }
        self.allocator.set_silo_affinity(silo_id, node_id);
    }

    pub fn alloc_page(&mut self, silo_id: u64) -> Option<u64> {
        self.allocator.alloc_page(silo_id)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NumaAllocBridge: sets={} imbalance_warnings={}",
            self.stats.affinity_sets, self.stats.imbalance_warnings
        );
    }
}
