//! # NUMA Memory Affinity Bridge (Phase 199)
//!
//! ## Architecture Guardian: The Gap
//! `numa.rs` implements `NumaManager`:
//! - `add_node(id, cpus: Vec<u32>, memory: u64)` — register NUMA node
//! - `set_distance(from, to, distance)` — configure inter-node latency
//!
//! **Missing link**: NUMA node assignment was never tied to Silo affinity.
//! A Silo allocated on a remote NUMA node suffered 4-10× memory latency
//! without the kernel tracking or optimizing this placement.
//!
//! This module provides `NumaAffinityBridge`:
//! Tracks Silo → NUMA node binding, logs memory locality score.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::numa::NumaManager;

#[derive(Debug, Default, Clone)]
pub struct NumaAffinityStats {
    pub bindings:          u64,
    pub remote_accesses:   u64,
    pub local_accesses:    u64,
}

pub struct NumaAffinityBridge {
    pub manager:      NumaManager,
    silo_numa_node:  BTreeMap<u64, u32>,  // silo_id → numa_node_id
    pub stats:        NumaAffinityStats,
}

impl NumaAffinityBridge {
    pub fn new() -> Self {
        NumaAffinityBridge { manager: NumaManager::new(), silo_numa_node: BTreeMap::new(), stats: NumaAffinityStats::default() }
    }

    /// Bind a Silo to a NUMA node for memory locality.
    pub fn bind_silo(&mut self, silo_id: u64, numa_node: u32) {
        self.silo_numa_node.insert(silo_id, numa_node);
        self.stats.bindings += 1;
        crate::serial_println!("[NUMA] Silo {} bound to node {}", silo_id, numa_node);
    }

    /// Check if a memory access is local/remote for a Silo and log it.
    pub fn record_access(&mut self, silo_id: u64, phys_addr_node: u32) {
        let home_node = *self.silo_numa_node.get(&silo_id).unwrap_or(&0);
        if phys_addr_node == home_node {
            self.stats.local_accesses += 1;
        } else {
            self.stats.remote_accesses += 1;
        }
    }

    /// Get approximate locality score (0-100): 100 = all local.
    pub fn locality_score(&self) -> u8 {
        let total = self.stats.local_accesses + self.stats.remote_accesses;
        if total == 0 { return 100; }
        ((self.stats.local_accesses * 100) / total) as u8
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NumaBridge: bindings={} local={} remote={} score={}",
            self.stats.bindings, self.stats.local_accesses,
            self.stats.remote_accesses, self.locality_score()
        );
    }
}
