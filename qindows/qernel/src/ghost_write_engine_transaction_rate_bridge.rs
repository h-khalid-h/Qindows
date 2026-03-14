//! # Ghost Write Engine Transaction Rate Bridge (Phase 285)
//!
//! ## Architecture Guardian: The Gap
//! `ghost_write_engine.rs` implements `GwTransaction`:
//! - `GwTransaction { phase: GwTxPhase, ops: Vec<GwWriteOp>, ... }`
//! - `GwWriteOp::compute_oid()` — computes content-addressed ID
//! - `ShadowObject::can_free()` → bool — safe to GC
//!
//! **Missing link**: GwTransaction operation count was unbounded.
//! A transaction with millions of GwWriteOp entries would block the
//! ghost write pipeline for seconds, starving other write operations.
//!
//! This module provides `GhostWriteEngineTransactionRateBridge`:
//! Max 1024 write ops per GwTransaction.

extern crate alloc;

const MAX_OPS_PER_TRANSACTION: u64 = 1024;

#[derive(Debug, Default, Clone)]
pub struct GwTransactionRateStats {
    pub txns_allowed: u64,
    pub txns_denied:  u64,
}

pub struct GhostWriteEngineTransactionRateBridge {
    pub stats: GwTransactionRateStats,
}

impl GhostWriteEngineTransactionRateBridge {
    pub fn new() -> Self {
        GhostWriteEngineTransactionRateBridge { stats: GwTransactionRateStats::default() }
    }

    pub fn authorize_transaction(&mut self, op_count: u64, silo_id: u64) -> bool {
        if op_count > MAX_OPS_PER_TRANSACTION {
            self.stats.txns_denied += 1;
            crate::serial_println!(
                "[GHOST WRITE] Silo {} transaction op_count {} exceeds cap {}",
                silo_id, op_count, MAX_OPS_PER_TRANSACTION
            );
            return false;
        }
        self.stats.txns_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GwTransactionRateBridge: allowed={} denied={}",
            self.stats.txns_allowed, self.stats.txns_denied
        );
    }
}
