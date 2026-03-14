//! # Identity TPM Audit Bridge (Phase 205)
//!
//! ## Architecture Guardian: The Gap
//! `identity.rs` implements Q-Identity and hardware attestation.
//! The existing `identity_tpm_bridge.rs` connects to TPM.
//!
//! `collab.rs` implements `VectorClock` / `CrdtOp` for collaborative editing.
//! 
//! **Missing link**: Collaborative editing sessions (collab CRDT ops)
//! were never gated by capability check. Any Silo could inject CRDT ops
//! into another Silo's collaborative document — a Law 9 data violation.
//!
//! This module provides `CollabCrdtCapBridge`:
//! Prism:WRITE cap required before any CRDT op is applied to a session.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_WRITE, CAP_READ};

#[derive(Debug, Default, Clone)]
pub struct CollabCrdtCapStats {
    pub ops_allowed:  u64,
    pub ops_denied:   u64,
    pub reads_allowed: u64,
    pub reads_denied:  u64,
}

pub struct CollabCrdtCapBridge {
    pub stats: CollabCrdtCapStats,
}

impl CollabCrdtCapBridge {
    pub fn new() -> Self {
        CollabCrdtCapBridge { stats: CollabCrdtCapStats::default() }
    }

    /// Authorize writing a CRDT op to a session — requires Prism:WRITE cap.
    pub fn authorize_write(
        &mut self,
        silo_id: u64,
        session_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_WRITE, 0, tick) {
            self.stats.ops_denied += 1;
            crate::serial_println!(
                "[COLLAB CRDT] Silo {} write to session {} DENIED — no Prism:WRITE cap", silo_id, session_id
            );
            return false;
        }
        self.stats.ops_allowed += 1;
        true
    }

    /// Authorize reading from a session — requires Prism:READ cap.
    pub fn authorize_read(
        &mut self,
        silo_id: u64,
        session_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_READ, 0, tick) {
            self.stats.reads_denied += 1;
            crate::serial_println!(
                "[COLLAB CRDT] Silo {} read session {} DENIED — no Prism:READ cap", silo_id, session_id
            );
            return false;
        }
        self.stats.reads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CollabCrdtBridge: write={}/{} read={}/{}",
            self.stats.ops_allowed, self.stats.ops_denied,
            self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
