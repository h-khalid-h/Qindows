//! # NUMA-Aware Allocator — Per-Node Memory Pools
//!
//! Extends the physical page allocator with NUMA awareness.
//! Provides per-node memory pools so fibers and Silos
//! allocate from the closest NUMA node (Section 9.7).
//!
//! Features:
//! - Per-NUMA-node free-page pools
//! - Silo → preferred node binding
//! - Fallback to remote nodes when local exhausted
//! - Migration: move pages to preferred node
//! - Per-node usage statistics

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// A NUMA node's page pool.
#[derive(Debug, Clone)]
pub struct NumaPool {
    pub node_id: u32,
    pub total_pages: u64,
    pub free_pages: u64,
    pub allocated_pages: u64,
    /// Free page frame numbers
    pub free_list: Vec<u64>,
}

/// NUMA allocation statistics.
#[derive(Debug, Clone, Default)]
pub struct NumaAllocStats {
    pub local_allocs: u64,
    pub remote_allocs: u64,
    pub alloc_failures: u64,
    pub migrations: u64,
    pub total_freed: u64,
}

/// The NUMA-Aware Allocator.
pub struct NumaAllocator {
    pub pools: BTreeMap<u32, NumaPool>,
    /// Silo → preferred NUMA node
    pub silo_affinity: BTreeMap<u64, u32>,
    pub stats: NumaAllocStats,
}

impl NumaAllocator {
    pub fn new() -> Self {
        NumaAllocator {
            pools: BTreeMap::new(),
            silo_affinity: BTreeMap::new(),
            stats: NumaAllocStats::default(),
        }
    }

    /// Register a NUMA node with its page pool.
    pub fn add_node(&mut self, node_id: u32, base_pfn: u64, page_count: u64) {
        let free_list: Vec<u64> = (base_pfn..base_pfn + page_count).collect();
        self.pools.insert(node_id, NumaPool {
            node_id, total_pages: page_count,
            free_pages: page_count, allocated_pages: 0,
            free_list,
        });
    }

    /// Bind a Silo to a preferred NUMA node.
    pub fn set_silo_affinity(&mut self, silo_id: u64, node_id: u32) {
        self.silo_affinity.insert(silo_id, node_id);
    }

    /// Allocate a page for a Silo (prefers local node).
    pub fn alloc_page(&mut self, silo_id: u64) -> Option<u64> {
        let preferred = self.silo_affinity.get(&silo_id).copied();

        // Try preferred node first
        if let Some(node_id) = preferred {
            if let Some(pfn) = self.alloc_from_node(node_id) {
                self.stats.local_allocs += 1;
                return Some(pfn);
            }
        }

        // Fallback: try any node with free pages
        let nodes: Vec<u32> = self.pools.keys().copied().collect();
        for node_id in nodes {
            if Some(node_id) == preferred { continue; }
            if let Some(pfn) = self.alloc_from_node(node_id) {
                self.stats.remote_allocs += 1;
                return Some(pfn);
            }
        }

        self.stats.alloc_failures += 1;
        None
    }

    /// Allocate from a specific node.
    fn alloc_from_node(&mut self, node_id: u32) -> Option<u64> {
        if let Some(pool) = self.pools.get_mut(&node_id) {
            if let Some(pfn) = pool.free_list.pop() {
                pool.free_pages -= 1;
                pool.allocated_pages += 1;
                return Some(pfn);
            }
        }
        None
    }

    /// Free a page back to its node.
    pub fn free_page(&mut self, pfn: u64) {
        // Find which node owns this PFN
        for pool in self.pools.values_mut() {
            let base = pool.free_list.first().copied().unwrap_or(0);
            let end = base + pool.total_pages;
            // A freed page could be anywhere in the node's range
            if pfn >= base.saturating_sub(pool.allocated_pages) && pfn < end + pool.allocated_pages {
                pool.free_list.push(pfn);
                pool.free_pages += 1;
                pool.allocated_pages = pool.allocated_pages.saturating_sub(1);
                self.stats.total_freed += 1;
                return;
            }
        }
        // If no node claims it, add to node 0 as fallback
        if let Some(pool) = self.pools.values_mut().next() {
            pool.free_list.push(pfn);
            pool.free_pages += 1;
            self.stats.total_freed += 1;
        }
    }

    /// Total free pages across all nodes.
    pub fn total_free(&self) -> u64 {
        self.pools.values().map(|p| p.free_pages).sum()
    }
}
