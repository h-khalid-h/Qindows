//! # Object Shard Prism Bridge (Phase 153)
//!
//! ## Architecture Guardian: The Gap
//! `object_shard.rs` implements `ObjectShardEngine`:
//! - `shard(oid, size, config, peer_nodes, tick)` → Option<&ShardSet>
//! - `confirm_shard(oid, index)` — marks stored
//! - `report_lost(oid, index)` — marks lost (schedules re-replication)
//!
//! **Missing link**: `PrismStoreBridge` wrote objects locally but never
//! triggered distributed sharding for large objects.
//!
//! This module provides `ObjectShardPrismBridge`:
//! 1. `write_with_sharding()` — if object ≥ 1MiB + peers available → shard
//! 2. `confirm_shard_stored()` / `report_shard_loss()` — lifecycle

extern crate alloc;
use alloc::vec::Vec;

use crate::object_shard::{ObjectShardEngine, ShardConfig};

const SHARD_THRESHOLD_BYTES: u64 = 1024 * 1024; // 1 MiB

#[derive(Debug, Default, Clone)]
pub struct ShardBridgeStats {
    pub objects_sharded:  u64,
    pub objects_local:    u64,
    pub shards_confirmed: u64,
    pub shards_lost:      u64,
}

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

    /// Called after a Prism write. If large, shard across peers.
    /// Returns Vec of (shard_index, holder_node) for Nexus delivery.
    pub fn write_with_sharding(
        &mut self,
        oid: [u8; 32],
        data_size: u64,
        peer_nodes: &[u64],
        tick: u64,
    ) -> Vec<(u8, u64)> {
        if data_size < SHARD_THRESHOLD_BYTES || peer_nodes.is_empty() {
            self.stats.objects_local += 1;
            return Vec::new();
        }

        self.stats.objects_sharded += 1;

        // shard() takes 5 args and returns Option<&ShardSet>
        if let Some(shard_set) = self.engine.shard(oid, data_size, ShardConfig::STANDARD, peer_nodes, tick) {
            let targets: Vec<(u8, u64)> = shard_set
                .shards
                .iter()
                .map(|s| (s.shard_index, s.holder_node))
                .collect();

            crate::serial_println!(
                "[SHARD] OID {:02x}{:02x}.. ({} bytes) → {} shards",
                oid[0], oid[1], data_size, targets.len()
            );
            targets
        } else {
            crate::serial_println!("[SHARD] Not enough peers to shard OID — stored locally");
            self.stats.objects_local += 1;
            Vec::new()
        }
    }

    pub fn confirm_shard_stored(&mut self, oid: &[u8; 32], shard_index: u8) {
        self.stats.shards_confirmed += 1;
        self.engine.confirm_shard(oid, shard_index);
    }

    pub fn report_shard_loss(&mut self, oid: &[u8; 32], shard_index: u8) {
        self.stats.shards_lost += 1;
        self.engine.report_lost(oid, shard_index, None, 0);
        crate::serial_println!(
            "[SHARD] LOSS — OID {:02x}{:02x}.. shard #{}", oid[0], oid[1], shard_index
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
