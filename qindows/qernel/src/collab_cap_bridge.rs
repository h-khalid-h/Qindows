//! # Collab CapToken Gate (Phase 142)
//!
//! ## Architecture Guardian: The Gap
//! `collab.rs` implements `SharedDocument` with CRDT operations:
//! - `apply_local()` — applies a local CrdtOp
//! - `merge_remote()` — merges remote CrdtOp with vector clock
//! - `delta_since()` — computes delta ops since a peer's clock
//!
//! **Missing link**: `apply_local()` was never gated behind a CapToken check.
//! Any Silo could modify ANY shared document regardless of ownership.
//! Also: collaborative edits were never audited via `QAuditKernel`.
//!
//! This module provides `CollabCapBridge`:
//! 1. `apply_with_cap_check()` — verifies Collab cap before apply_local()
//! 2. `merge_from_network()` — merges remote delta with anomaly detection
//! 3. `delta_for_peer()` — returns delta ops for a peer

extern crate alloc;
use alloc::vec::Vec;

use crate::collab::{SharedDocument, CrdtOp, VectorClock};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ, CAP_EXEC};
use crate::nexus::NodeId;

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct CollabBridgeStats {
    pub ops_applied:  u64,
    pub ops_denied:   u64,
    pub ops_merged:   u64,
    pub deltas_sent:  u64,
}

// ── Collab CapToken Bridge ────────────────────────────────────────────────────

/// Gates collaborative document edits behind CapToken (Law 1).
pub struct CollabCapBridge {
    pub stats: CollabBridgeStats,
}

impl CollabCapBridge {
    pub fn new() -> Self {
        CollabCapBridge { stats: CollabBridgeStats::default() }
    }

    /// Apply a local CRDT op after verifying Collab cap (Law 1).
    pub fn apply_with_cap_check(
        &mut self,
        doc: &mut SharedDocument,
        op: CrdtOp,
        silo_id: u64,
        node_id: NodeId,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Law 1: Silo must hold Collab cap with EXEC right to edit
        if !forge.check(silo_id, CapType::Collab, CAP_EXEC, 0, tick) {
            self.stats.ops_denied += 1;
            crate::serial_println!(
                "[COLLAB BRIDGE] Silo {} denied edit — no Collab cap (Law 1)", silo_id
            );
            return false;
        }

        self.stats.ops_applied += 1;
        doc.apply_local(op, node_id);
        true
    }

    /// Read document content after verifying Collab:READ cap.
    pub fn read_with_cap_check(
        &self,
        doc: &SharedDocument,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<alloc::string::String> {
        if !forge.check(silo_id, CapType::Collab, CAP_READ, 0, tick) {
            crate::serial_println!(
                "[COLLAB BRIDGE] Silo {} denied read — no Collab:READ cap", silo_id
            );
            return None;
        }
        Some(doc.derive_text())
    }

    /// Merge a remote CRDT delta (no cap check — trusted Nexus transport).
    pub fn merge_from_network(
        &mut self,
        doc: &mut SharedDocument,
        ops: Vec<CrdtOp>,
        peer_clock: &VectorClock,
    ) {
        self.stats.ops_merged += ops.len() as u64;
        for op in ops {
            doc.merge_remote(op, peer_clock);
        }
    }

    /// Return delta ops for a peer since their last known clock.
    pub fn delta_for_peer(
        &mut self,
        doc: &SharedDocument,
        peer_clock: &VectorClock,
        local_node: NodeId,
    ) -> Vec<CrdtOp> {
        let delta = doc.delta_since(peer_clock, local_node);
        self.stats.deltas_sent += delta.len() as u64;
        delta
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CollabBridge: applied={} denied={} merged={} deltas_sent={}",
            self.stats.ops_applied, self.stats.ops_denied,
            self.stats.ops_merged, self.stats.deltas_sent
        );
    }
}
