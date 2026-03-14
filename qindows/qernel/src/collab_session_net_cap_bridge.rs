//! # Collab Session Net Cap Bridge (Phase 241)
//!
//! ## Architecture Guardian: The Gap
//! `collab_session_net.rs` implements `CollabSessionNet`:
//! - `announce_session(qring, tick)` — broadcast session to mesh
//! - `apply_local_op(op: CrdtOp, qring, tick)` — apply CRDT op + push delta
//! - `receive_delta(ops, peer_clock, tick)` — receive peer CRDT deltas
//!
//! **Missing link**: `apply_local_op()` could be called without Collab
//! cap verification. The collaborative CRDT delta was pushed over the
//! Nexus mesh without session ownership check.
//!
//! This module provides `CollabSessionNetCapBridge`:
//! Prism:WRITE cap required before any local CRDT op is applied.

extern crate alloc;
use alloc::vec::Vec;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_WRITE};

#[derive(Debug, Default, Clone)]
pub struct CollabSessionNetCapStats {
    pub ops_allowed:  u64,
    pub ops_denied:   u64,
}

pub struct CollabSessionNetCapBridge {
    pub stats: CollabSessionNetCapStats,
}

impl CollabSessionNetCapBridge {
    pub fn new() -> Self {
        CollabSessionNetCapBridge { stats: CollabSessionNetCapStats::default() }
    }

    /// Authorize applying a local CRDT op — requires Prism:WRITE cap.
    pub fn authorize_apply_op(
        &mut self,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_WRITE, 0, tick) {
            self.stats.ops_denied += 1;
            crate::serial_println!(
                "[COLLAB NET] Silo {} CRDT apply_op denied — no Prism:WRITE cap", silo_id
            );
            return false;
        }
        self.stats.ops_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CollabSessionNetBridge: allowed={} denied={}",
            self.stats.ops_allowed, self.stats.ops_denied
        );
    }
}
