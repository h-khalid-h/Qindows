//! # Object Shard Prism Bridge (Phase 153)
//!
//! ## Architecture Guardian: The Gap
//! `object_shard.rs` implements `ObjectShardEngine`:
//! - `shard(oid, size, config, peer_nodes)` — splits object into erasure-coded shards
//! - `confirm_shard(oid, index)` — marks a shard as confirmed stored
//! - `report_lost(oid, index)` — marks a shard as lost
//!
//! **Missing link**: Sharding was implemented but never triggered when
//! `PrismStoreBridge` wrote a large object. Objects below the shard threshold
//! went through ghost_write, but none of the large objects ever triggered
//! distributed sharding across Nexus peers.
//!
//! This module provides `ObjectShardPrismBridge`:
//! 1. `write_with_sharding()` — if object ≥ threshold, shard across peers
//! 2. `confirm_shard_stored()` — mark shard confirmed after Nexus delivery
//! 3. `report_shard_loss()` — trigger replication recovery

extern crate alloc;
use alloc::vec::Vec;

use crate::object_shard::{ObjectShardEngine, ShardConfig};

/// Minimum object size (bytes) to trigger distributed sharding.
const SHARD_THRESHOLD_BYTES: u64 = 1024 * 1024; // 1 MiB

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ShardBridgeStats {
    pub objects_sharded:   u64,
    pub objects_local:     u64,
    pub shards_confirmed:  u64,
    pub shards_lost:       u64,
}

// ── Object Shard Prism Bridge ─────────────────────────────────────────────────

/// Decides whether an object should be sharded across Nexus peers.
pub struct ObjectShardPrismBridge {
    pub engine: ObjectShardEngine,
    pub stats:  ShardBridgeStats,
}

impl ObjectShardPrismBridge {
    pub fn new() -> Self {
        ObjectShardPrismBridge {
            engine: ObjectShardEngine::new(),
            stats:  ShardBridgeStats::default(),
        }
    }

    /// Called after a Prism object write. If large, shard across peers.
    /// Returns the shard targets (Vec of (shard_index, node_id)) or empty (local only).
    pub fn write_with_sharding(
        &mut self,
        oid: [u8; 32],
        data_size: u64,
        peer_nodes: &[u64],
    ) -> Vec<(u8, u64)> {
        if data_size < SHARD_THRESHOLD_BYTES || peer_nodes.is_empty() {
            self.stats.objects_local += 1;
            return Vec::new();
        }

        self.stats.objects_sharded += 1;

        let shard_set = self.engine.shard(oid, data_size, ShardConfig::STANDARD, peer_nodes);

        let targets: Vec<(u8, u64)> = shard_set
            .shards
            .iter()
            .map(|s| (s.index, s.holder_node))
            .collect();

        crate::serial_println!(
            "[SHARD BRIDGE] OID {:02x}{:02x}.. ({} bytes) → {} shards across {} peers",
            oid[0], oid[1], data_size, targets.len(), peer_nodes.len()
        );

        targets
    }

    /// Mark a shard as successfully stored on its target node.
    pub fn confirm_shard_stored(&mut self, oid: &[u8; 32], shard_index: u8) {
        self.stats.shards_confirmed += 1;
        self.engine.confirm_shard(oid, shard_index);
    }

    /// Report a shard as lost; engine schedules re-replication.
    pub fn report_shard_loss(&mut self, oid: &[u8; 32], shard_index: u8) {
        self.stats.shards_lost += 1;
        self.engine.report_lost(oid, shard_index);
        crate::serial_println!(
            "[SHARD BRIDGE] SHARD LOSS — OID {:02x}{:02x}.. shard #{}", oid[0], oid[1], shard_index
        );
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ShardBridge: sharded={} local={} confirmed={} lost={}",
            self.stats.objects_sharded, self.stats.objects_local,
            self.stats.shards_confirmed, self.stats.shards_lost
        );
    }
}
