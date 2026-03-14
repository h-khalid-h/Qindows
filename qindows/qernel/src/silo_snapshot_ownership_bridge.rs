//! # Silo Snapshot Ownership Bridge (Phase 249)
//!
//! ## Architecture Guardian: The Gap
//! `silo_snapshot.rs` implements `SnapshotManager`:
//! - `SiloSnapshot { silo_id, snap_state, page_deltas, file_refs, ... }`
//! - `SnapState` — Active, Frozen, Checkpoint, Failed
//!
//! **Missing link**: Snapshot access checks were done at restore time
//! only. A Silo could read the `page_deltas` of another Silo's snapshot,
//! leaking in-memory state (page data, file refs, thread contexts).
//!
//! This module provides `SiloSnapshotOwnershipBridge`:
//! Snapshot read requires caller_silo == snapshot.silo_id or Admin:EXEC.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};
use crate::silo_snapshot::SiloSnapshot;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct SnapshotOwnershipStats {
    pub reads_allowed: u64,
    pub reads_denied:  u64,
}

pub struct SiloSnapshotOwnershipBridge {
    pub stats: SnapshotOwnershipStats,
}

impl SiloSnapshotOwnershipBridge {
    pub fn new() -> Self {
        SiloSnapshotOwnershipBridge { stats: SnapshotOwnershipStats::default() }
    }

    /// Authorize snapshot read — owner always allowed, others need Admin:EXEC.
    pub fn authorize_read(
        &mut self,
        reader_silo: u64,
        snapshot: &SiloSnapshot,
        forge: &mut CapTokenForge,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if reader_silo == snapshot.silo_id {
            self.stats.reads_allowed += 1;
            return true;
        }
        if !forge.check(reader_silo, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.reads_denied += 1;
            audit.log_law_violation(6u8, reader_silo, tick);
            crate::serial_println!(
                "[SNAPSHOT] Silo {} denied reading Silo {} snapshot — no Admin:EXEC (Law 6)",
                reader_silo, snapshot.silo_id
            );
            return false;
        }
        self.stats.reads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SnapshotOwnershipBridge: allowed={} denied={}",
            self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
