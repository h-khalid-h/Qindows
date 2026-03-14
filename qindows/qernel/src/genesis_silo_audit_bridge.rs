//! # Genesis Silo Audit Bridge (Phase 229)
//!
//! ## Architecture Guardian: The Gap
//! `genesis.rs` implements the initial Silo creation sequence.
//! The genesis bootstrap creates the first privileged Silo with
//! Root-level capabilities (Admin + all types).
//!
//! **Missing link**: Genesis did not log the CapType grants made to
//! the initial system Silos. The earliest privilege grants happened
//! before the audit system was initialized — a Law 1 blind spot.
//!
//! This module provides `GenesisSiloAuditBridge`:
//! Retroactive audit log of all genesis-phase cap grants.

extern crate alloc;
use alloc::vec::Vec;

use crate::cap_tokens::CapType;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct GenesisAuditStats {
    pub grants_logged: u64,
}

pub struct GenesisSiloAuditBridge {
    pub stats: GenesisAuditStats,
}

impl GenesisSiloAuditBridge {
    pub fn new() -> Self {
        GenesisSiloAuditBridge { stats: GenesisAuditStats::default() }
    }

    /// Log that a genesis-phase CapType grant was made to a Silo (retroactive audit).
    pub fn log_cap_grant(
        &mut self,
        silo_id: u64,
        cap_type: &CapType,
        audit: &mut QAuditKernel,
        tick: u64,
    ) {
        self.stats.grants_logged += 1;
        // Law 1: all cap grants must be audited, even at genesis
        audit.log_law_violation(1u8, silo_id, tick);
        crate::serial_println!(
            "[GENESIS AUDIT] Silo {} granted {:?} at genesis (Law 1 retroactive)", silo_id, cap_type
        );
    }

    /// Log batch genesis cap grants for a set of initial Silos.
    pub fn log_genesis_silos(
        &mut self,
        silos: &[(u64, Vec<CapType>)],
        audit: &mut QAuditKernel,
        tick: u64,
    ) {
        for (silo_id, caps) in silos {
            for cap in caps {
                self.log_cap_grant(*silo_id, cap, audit, tick);
            }
        }
        crate::serial_println!("[GENESIS AUDIT] {} total cap grants logged", self.stats.grants_logged);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GenesisSiloAuditBridge: grants_logged={}", self.stats.grants_logged
        );
    }
}
