//! # QLedger Entry Integrity Bridge (Phase 220)
//!
//! ## Architecture Guardian: The Gap
//! `qledger.rs` implements the Q-Ledger distributed ledger:
//! - Immutable append-only log of kernel-verified transactions
//! - Each entry: silo_id, operation, hash, prev_hash
//!
//! **Missing link**: Ledger entries were written without verifying that
//! the hash chain was unbroken. A corrupted or tampered prev_hash could
//! silently break ledger integrity without detection.
//!
//! This module provides `QLedgerIntegrityBridge`:
//! Validates prev_hash continuity before each entry is appended.

extern crate alloc;

use crate::crypto_primitives::sha256;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QLedgerIntegrityStats {
    pub entries_ok:      u64,
    pub chain_failures:  u64,
}

pub struct QLedgerIntegrityBridge {
    pub last_hash: [u8; 32],
    pub stats:     QLedgerIntegrityStats,
}

impl QLedgerIntegrityBridge {
    pub fn new() -> Self {
        QLedgerIntegrityBridge { last_hash: [0u8; 32], stats: QLedgerIntegrityStats::default() }
    }

    /// Verify that an entry's prev_hash matches the last committed hash.
    /// Returns false if chain is broken (tamper detected).
    pub fn verify_chain(
        &mut self,
        prev_hash: &[u8; 32],
        entry_data: &[u8],
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if prev_hash != &self.last_hash {
            self.stats.chain_failures += 1;
            audit.log_law_violation(9u8, 0, tick); // Law 9: data integrity
            crate::serial_println!("[QLEDGER] Chain break detected! hash mismatch — Law 9 audit");
            return false;
        }
        // Advance the chain
        let new_hash = sha256(entry_data);
        self.last_hash = new_hash;
        self.stats.entries_ok += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QLedgerBridge: ok={} chain_failures={}",
            self.stats.entries_ok, self.stats.chain_failures
        );
    }
}
