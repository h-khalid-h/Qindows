//! # Object Shard Integrity Bridge (Phase 239)
//!
//! ## Architecture Guardian: The Gap
//! `object_shard.rs` implements distributed object sharding:
//! - `ShardSet::is_recoverable()` → bool — is data recoverable from available shards?
//! - `ShardSet::fault_tolerance()` → u8 — how many shards can fail
//! - `ShardKind` — Data, Parity, Repair, Metadata
//!
//! **Missing link**: ShardSet recovery was not audited. Attempted recovery
//! from a degraded shard set (below `min_for_recovery()` threshold) was
//! silent — data corruption could occur without detection.
//!
//! This module provides `ObjectShardIntegrityBridge`:
//! Audits degraded shard recovery attempts (Law 9: data integrity).

extern crate alloc;

use crate::object_shard::ShardSet;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct ShardIntegrityStats {
    pub recoveries_ok:       u64,
    pub recoveries_degraded: u64,
}

pub struct ObjectShardIntegrityBridge {
    pub stats: ShardIntegrityStats,
}

impl ObjectShardIntegrityBridge {
    pub fn new() -> Self {
        ObjectShardIntegrityBridge { stats: ShardIntegrityStats::default() }
    }

    /// Check shard set health before recovery — audits on degraded state.
    pub fn check_recovery(
        &mut self,
        shard_set: &ShardSet,
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if shard_set.is_recoverable() {
            self.stats.recoveries_ok += 1;
            true
        } else {
            self.stats.recoveries_degraded += 1;
            audit.log_law_violation(9u8, silo_id, tick); // Law 9: data integrity
            crate::serial_println!(
                "[OBJECT SHARD] Silo {} shard set below threshold — recovery unsafe (Law 9)",
                silo_id
            );
            false
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ObjectShardBridge: ok={} degraded={}",
            self.stats.recoveries_ok, self.stats.recoveries_degraded
        );
    }
}
