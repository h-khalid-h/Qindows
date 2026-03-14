//! # Q-Ring Hardening Bridge (Phase 136)
//!
//! ## Architecture Guardian: The Gap
//! `qring_guard.rs` (Phase 85) implements:
//! - `validate_slot_index()` — validates a ring slot index
//! - `validate_ring_depth()` — validates that ring depth is power-of-2
//! - `validate_ring_syscall()` — validates syscall_id against allowed set
//! - `harden_qring_batch()` — validates a batch of SqEntries before dispatch
//!
//! **Missing link**: `harden_qring_batch()` was implemented but never called
//! from `qring_async::dispatch()` or `syscall_table::dispatch()`. Every
//! Q-Ring submission bypassed the hardening checks.
//!
//! This module provides `QRingHardeningBridge`:
//! 1. `harden_before_dispatch()` — calls harden_qring_batch() on every batch
//! 2. `validate_silo_ring()` — checks ring depth + slot alignment for a Silo
//! 3. `report_hardening_stats()` — exposes harden stats for q_admin_bridge

extern crate alloc;
use alloc::vec::Vec;

use crate::qring_guard::{
    harden_qring_batch, validate_ring_depth, validate_ring_syscall,
    validate_slot_index, SlotValidation, SyscallHardeningStats,
};
// Note: harden_qring_batch works on parallel index/syscall_id slices, not SqEntry
// SqEntry is not imported since harden works on raw u64 arrays.

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct HardeningBridgeStats {
    pub batches_hardened:   u64,
    pub entries_valid:      u64,
    pub entries_rejected:   u64,
    pub rings_validated:    u64,
    pub rings_invalid:      u64,
}

// ── QRing Hardening Bridge ────────────────────────────────────────────────────

/// Interposes batch hardening and ring validation before Q-Ring dispatch.
pub struct QRingHardeningBridge {
    pub stats: HardeningBridgeStats,
    pub hardening_stats: SyscallHardeningStats,
}

impl QRingHardeningBridge {
    pub fn new() -> Self {
        QRingHardeningBridge {
            stats: HardeningBridgeStats::default(),
            hardening_stats: SyscallHardeningStats::default(),
        }
    }

    /// Harden parallel slot-index and syscall-id arrays before dispatch.
    /// Returns (valid_count, rejected_count).
    pub fn harden_before_dispatch(
        &mut self,
        indices: &[u64],
        syscall_ids: &[u64],
        ring_depth: u64,
        silo_id: u64,
        _tick: u64,
    ) -> (usize, usize) {
        self.stats.batches_hardened += 1;

        let (valid, rejected) = harden_qring_batch(indices, syscall_ids, ring_depth);

        self.stats.entries_valid    += valid as u64;
        self.stats.entries_rejected += rejected as u64;

        if rejected > 0 {
            self.hardening_stats.unknown_syscalls += rejected as u64;
            crate::serial_println!(
                "[QRING HARD] Silo {} batch: {}/{} valid, {} rejected",
                silo_id, valid, indices.len(), rejected
            );
        }

        (valid, rejected)
    }

    /// Validate that a Silo's Q-Ring depth and capacity are legal.
    pub fn validate_silo_ring(&mut self, ring_depth: u64) -> Result<u64, &'static str> {
        self.stats.rings_validated += 1;
        let result = validate_ring_depth(ring_depth);
        if result.is_err() {
            self.stats.rings_invalid += 1;
        }
        result
    }

    /// Validate a single slot index for bounds checking.
    pub fn validate_slot(&self, raw_index: u64, ring_depth: u64) -> bool {
        matches!(validate_slot_index(raw_index, ring_depth), SlotValidation::Ok(_))
    }

    /// Validate a syscall ID before dispatch.
    pub fn validate_syscall_id(&self, syscall_id: u64) -> bool {
        validate_ring_syscall(syscall_id).is_ok()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QRingHarden: batches={} valid={} rejected={} rings_ok={} rings_bad={}",
            self.stats.batches_hardened, self.stats.entries_valid,
            self.stats.entries_rejected, self.stats.rings_validated, self.stats.rings_invalid
        );
    }
}
