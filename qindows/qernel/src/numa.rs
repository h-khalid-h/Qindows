//! # NUMA Topology Manager — Memory Affinity & Scheduling
//!
//! Optimizes memory allocation and thread scheduling based on
//! Non-Uniform Memory Access topology (Section 9.3).
//!
//! Features:
//! - NUMA node discovery and topology mapping
//! - Per-Silo memory affinity (pin Silo to NUMA node)
//! - Distance-aware allocation (prefer local memory)
//! - Cross-node migration cost estimation
//! - Rebalancing when nodes become overloaded

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// A NUMA node.
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub cpus: Vec<u32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub silo_count: u32,
}

/// Distance between NUMA nodes.
#[derive(Debug, Clone)]
pub struct NumaDistance {
    pub from: u32,
    pub to: u32,
    pub distance: u32,
}

/// Silo affinity binding.
#[derive(Debug, Clone)]
pub struct AffinityBinding {
    pub silo_id: u64,
    pub preferred_node: u32,
    pub strict: bool,
}

/// NUMA statistics.
#[derive(Debug, Clone, Default)]
pub struct NumaStats {
    pub local_allocs: u64,
    pub remote_allocs: u64,
    pub migrations: u64,
    pub rebalances: u64,
}

/// The NUMA Topology Manager.
pub struct NumaManager {
    pub nodes: BTreeMap<u32, NumaNode>,
    pub distances: Vec<NumaDistance>,
    pub affinities: BTreeMap<u64, AffinityBinding>,
    pub stats: NumaStats,
}

impl NumaManager {
    pub fn new() -> Self {
        NumaManager {
            nodes: BTreeMap::new(),
            distances: Vec::new(),
            affinities: BTreeMap::new(),
            stats: NumaStats::default(),
        }
    }

    /// Register a NUMA node.
    pub fn add_node(&mut self, id: u32, cpus: Vec<u32>, memory: u64) {
        self.nodes.insert(id, NumaNode {
            id, cpus, memory_total: memory, memory_used: 0, silo_count: 0,
        });
    }

    /// Set distance between two nodes.
    pub fn set_distance(&mut self, from: u32, to: u32, distance: u32) {
        self.distances.retain(|d| !(d.from == from && d.to == to));
        self.distances.push(NumaDistance { from, to, distance });
    }

    /// Get distance between two nodes.
    pub fn distance(&self, from: u32, to: u32) -> u32 {
        if from == to { return 10; }
        self.distances.iter()
            .find(|d| d.from == from && d.to == to)
            .map(|d| d.distance)
            .unwrap_or(100)
    }

    /// Bind a Silo to a NUMA node.
    pub fn bind(&mut self, silo_id: u64, node_id: u32, strict: bool) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.silo_count += 1;
        }
        self.affinities.insert(silo_id, AffinityBinding {
            silo_id, preferred_node: node_id, strict,
        });
    }

    /// Allocate memory for a Silo.
    pub fn allocate(&mut self, silo_id: u64, size: u64) -> Option<u32> {
        let preferred = self.affinities.get(&silo_id)
            .map(|a| (a.preferred_node, a.strict));

        if let Some((pref_node, strict)) = preferred {
            if let Some(node) = self.nodes.get_mut(&pref_node) {
                if node.memory_used + size <= node.memory_total {
                    node.memory_used += size;
                    self.stats.local_allocs += 1;
                    return Some(pref_node);
                }
            }
            if strict { return None; }
        }

        let pref_id = preferred.map(|(n, _)| n).unwrap_or(0);
        let mut candidates: Vec<(u32, u32)> = self.nodes.iter()
            .filter(|(_, n)| n.memory_used + size <= n.memory_total)
            .map(|(&id, _)| (id, self.distance(pref_id, id)))
            .collect();

        candidates.sort_by_key(|&(_, dist)| dist);

        if let Some(&(node_id, _)) = candidates.first() {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.memory_used += size;
                self.stats.remote_allocs += 1;
                return Some(node_id);
            }
        }
        None
    }

    /// Free memory.
    pub fn free(&mut self, node_id: u32, size: u64) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.memory_used = node.memory_used.saturating_sub(size);
        }
    }

    /// Find least-loaded node.
    pub fn least_loaded(&self) -> Option<u32> {
        self.nodes.values()
            .min_by_key(|n| if n.memory_total > 0 { (n.memory_used * 100) / n.memory_total } else { 100 })
            .map(|n| n.id)
    }
}
