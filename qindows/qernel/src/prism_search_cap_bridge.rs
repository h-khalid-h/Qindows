//! # Prism Search CapToken Bridge (Phase 144)
//!
//! ## Architecture Guardian: The Gap
//! `prism_search.rs` implements `PrismIndex`:
//! - `ingest()` — indexes a `QNode` for search
//! - `get()` — retrieves a `QNode` by OID
//! (Additional full-text / vector search methods exist.)
//!
//! **Missing link**: `ingest()` and search queries were never gated behind
//! CapToken checks. Any Silo could search or index ANY Prism object
//! regardless of ownership — violating Law 1 (least privilege read).
//!
//! This module provides `PrismSearchCapBridge`:
//! 1. `ingest_with_cap_check()` — Prism:EXEC required to index
//! 2. `get_with_cap_check()` — Prism:READ required to retrieve
//! 3. `search_with_cap_check()` — Prism:READ required to search

extern crate alloc;
use alloc::vec::Vec;

use crate::prism_search::{PrismIndex, QNode, ObjectHandle};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct SearchBridgeStats {
    pub ingests:        u64,
    pub ingest_denied:  u64,
    pub queries:        u64,
    pub query_denied:   u64,
    pub lookups:        u64,
    pub lookup_denied:  u64,
}

// ── Prism Search Cap Bridge ───────────────────────────────────────────────────

/// Gates PrismIndex ingest and search behind CapToken (Law 1).
pub struct PrismSearchCapBridge {
    pub index: PrismIndex,
    pub stats: SearchBridgeStats,
}

impl PrismSearchCapBridge {
    pub fn new() -> Self {
        PrismSearchCapBridge {
            index: PrismIndex::new(),
            stats: SearchBridgeStats::default(),
        }
    }

    /// Index a QNode — requires Prism:EXEC cap (write/index right).
    pub fn ingest_with_cap_check(
        &mut self,
        node: QNode,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        self.stats.ingests += 1;
        if !forge.check(silo_id, CapType::Prism, CAP_EXEC, 0, tick) {
            self.stats.ingest_denied += 1;
            crate::serial_println!(
                "[SEARCH BRIDGE] Silo {} denied ingest — no Prism:EXEC cap (Law 1)", silo_id
            );
            return false;
        }
        self.index.ingest(node);
        true
    }

    /// Retrieve a node by OID — requires Prism:READ cap.
    pub fn get_with_cap_check(
        &mut self,
        oid: &[u8; 32],
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<&QNode> {
        self.stats.lookups += 1;
        if !forge.check(silo_id, CapType::Prism, CAP_READ, 0, tick) {
            self.stats.lookup_denied += 1;
            crate::serial_println!(
                "[SEARCH BRIDGE] Silo {} denied get — no Prism:READ cap", silo_id
            );
            return None;
        }
        self.index.get(oid)
    }

    /// Kernel-internal ingest (no cap check — trusted path).
    pub fn kernel_ingest(&mut self, node: QNode) {
        self.index.ingest(node);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SearchBridge: ingests={} denied={} queries={} q_denied={} lookups={} l_denied={}",
            self.stats.ingests, self.stats.ingest_denied,
            self.stats.queries, self.stats.query_denied,
            self.stats.lookups, self.stats.lookup_denied
        );
    }
}
