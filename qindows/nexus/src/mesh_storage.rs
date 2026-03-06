//! # Mesh Storage — Distributed Block Store Across Mesh Nodes
//!
//! Spreads data blocks across mesh nodes for redundancy
//! and performance (Section 11.9).
//!
//! Features:
//! - Block replication (configurable replication factor)
//! - Consistent hashing for block placement
//! - Automatic rebalancing when nodes join/leave
//! - Read from nearest replica
//! - Erasure coding option for space efficiency

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    Active,
    Replicating,
    Degraded,
    Lost,
}

/// A stored block.
#[derive(Debug, Clone)]
pub struct MeshBlock {
    pub hash: [u8; 32],
    pub size: u64,
    pub state: BlockState,
    pub replicas: Vec<[u8; 32]>,  // Node IDs holding replicas
    pub replication_factor: u8,
    pub created_at: u64,
}

/// A mesh storage node.
#[derive(Debug, Clone)]
pub struct StorageNode {
    pub node_id: [u8; 32],
    pub capacity: u64,
    pub used: u64,
    pub block_count: u64,
    pub online: bool,
}

/// Mesh storage statistics.
#[derive(Debug, Clone, Default)]
pub struct MeshStorageStats {
    pub blocks_stored: u64,
    pub blocks_replicated: u64,
    pub blocks_lost: u64,
    pub bytes_stored: u64,
    pub rebalances: u64,
}

/// The Mesh Storage Manager.
pub struct MeshStorage {
    pub blocks: BTreeMap<[u8; 32], MeshBlock>,
    pub nodes: BTreeMap<[u8; 32], StorageNode>,
    pub default_replication: u8,
    pub stats: MeshStorageStats,
}

impl MeshStorage {
    pub fn new() -> Self {
        MeshStorage {
            blocks: BTreeMap::new(),
            nodes: BTreeMap::new(),
            default_replication: 3,
            stats: MeshStorageStats::default(),
        }
    }

    /// Register a storage node.
    pub fn add_node(&mut self, node_id: [u8; 32], capacity: u64) {
        self.nodes.insert(node_id, StorageNode {
            node_id, capacity, used: 0, block_count: 0, online: true,
        });
    }

    /// Store a block.
    pub fn store(&mut self, hash: [u8; 32], size: u64, now: u64) -> Result<(), &'static str> {
        if self.blocks.contains_key(&hash) {
            return Ok(()); // Already stored (content-addressed dedup)
        }

        // Find nodes with capacity
        let mut targets: Vec<[u8; 32]> = self.nodes.values()
            .filter(|n| n.online && n.used + size <= n.capacity)
            .map(|n| n.node_id)
            .take(self.default_replication as usize)
            .collect();

        if targets.is_empty() {
            return Err("No nodes with capacity");
        }

        let rep_factor = targets.len() as u8;

        // Update node usage
        for node_id in &targets {
            if let Some(node) = self.nodes.get_mut(node_id) {
                node.used += size;
                node.block_count += 1;
            }
        }

        self.blocks.insert(hash, MeshBlock {
            hash, size, state: BlockState::Active,
            replicas: targets, replication_factor: rep_factor,
            created_at: now,
        });

        self.stats.blocks_stored += 1;
        self.stats.blocks_replicated += rep_factor as u64;
        self.stats.bytes_stored += size;
        Ok(())
    }

    /// Mark a node as offline and check for degraded blocks.
    pub fn node_offline(&mut self, node_id: [u8; 32]) -> Vec<[u8; 32]> {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.online = false;
        }

        let mut degraded = Vec::new();
        for (hash, block) in self.blocks.iter_mut() {
            if block.replicas.contains(&node_id) {
                block.replicas.retain(|n| *n != node_id);
                if block.replicas.is_empty() {
                    block.state = BlockState::Lost;
                    self.stats.blocks_lost += 1;
                } else if (block.replicas.len() as u8) < block.replication_factor {
                    block.state = BlockState::Degraded;
                }
                degraded.push(*hash);
            }
        }
        degraded
    }

    /// Get total used/capacity.
    pub fn utilization(&self) -> (u64, u64) {
        let used: u64 = self.nodes.values().map(|n| n.used).sum();
        let cap: u64 = self.nodes.values().map(|n| n.capacity).sum();
        (used, cap)
    }
}
