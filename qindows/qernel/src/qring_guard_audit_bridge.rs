//! # Q-Ring Guard Audit Bridge (Phase 177)
//!
//! ## Architecture Guardian: The Gap
//! `qring_guard.rs` provides:
//! - `validate_slot_index(raw_index, ring_depth)` → SlotValidation
//! - `validate_ring_depth(depth)` → Result<u64, &str>
//! - `validate_ring_syscall(syscall_id)` → Result<u64, &str>
//! - `harden_qring_batch(indices: &[u64], syscall_ids: &[u64], ring_depth: u64)` → (usize, usize)
//!
//! **Missing link**: `harden_qring_batch()` was never called from the
//! Q-Ring submission path. Crafted invalid slot indices could cause
//! out-of-bounds access in the kernel ring buffer.
//!
//! This module provides `QRingGuardAuditBridge`:
//! `submit_indices_batch()` — harden + audit any rejections via QAuditKernel.

extern crate alloc;

use crate::qring_guard::{harden_qring_batch, validate_ring_depth};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QRingGuardBridgeStats {
    pub batches_processed: u64,
    pub entries_rejected:  u64,
    pub entries_accepted:  u64,
}

pub struct QRingGuardAuditBridge {
    pub stats: QRingGuardBridgeStats,
}

impl QRingGuardAuditBridge {
    pub fn new() -> Self {
        QRingGuardAuditBridge { stats: QRingGuardBridgeStats::default() }
    }

    /// Harden a batch of Q-Ring submission indices + syscall IDs.
    /// Returns (valid, rejected) counts. Audits rejections to QAuditKernel.
    pub fn submit_indices_batch(
        &mut self,
        silo_id: u64,
        indices: &[u64],
        syscall_ids: &[u64],
        ring_depth: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> (usize, usize) {
        self.stats.batches_processed += 1;
        let (valid, rejected) = harden_qring_batch(indices, syscall_ids, ring_depth);

        if rejected > 0 {
            self.stats.entries_rejected += rejected as u64;
            audit.log_law_violation(6u8, silo_id, tick);
            crate::serial_println!(
                "[QRING GUARD] Silo {} batch: {} accepted, {} rejected", silo_id, valid, rejected
            );
        }
        self.stats.entries_accepted += valid as u64;
        (valid, rejected)
    }

    /// Validate ring depth before creating a new Q-Ring.
    pub fn validate_depth(&self, depth: u64) -> Result<u64, &'static str> {
        validate_ring_depth(depth)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QRingGuardBridge: batches={} accepted={} rejected={}",
            self.stats.batches_processed, self.stats.entries_accepted, self.stats.entries_rejected
        );
    }
}
