//! # Prism Live Object Index Bridge (Phase 114)
//!
//! ## Architecture Guardian: The Gap
//! `prism_query.rs` (Phase 93) provides `PrismQueryEngine::execute()` that
//! filters and sorts a `&[ObjectMeta]` slice.
//!
//! **The missing link**: `PrismQueryEngine::execute()` takes a pre-built
//! `&[ObjectMeta]` slice — but no code ever **built that slice** from the
//! live in-kernel object store. The kernel has no object index to query against!
//!
//! This module provides the **LiveObjectIndex** — an in-kernel registry of
//! every active `ObjectMeta` entry, populated by:
//! - Silo spawn (new Silo binary OID registered)
//! - Ghost-Write save (new object version created)
//! - Prism Cache eviction (objects entering cold storage)
//! - Silo vaporize (cleanup of Silo-owned objects)
//!
//! ## Design
//! From ARCHITECTURE.md: "Prism stores up to N=10 million objects; the kernel
//! maintains a hot index of the most recently accessed 64K objects."
//! The LiveObjectIndex is a ring-buffer of `ObjectMeta` entries that:
//! 1. Accepts new registrations from any kernel subsystem
//! 2. Evicts the oldest entries when full (LRU policy)
//! 3. Provides the `&[ObjectMeta]` slice for `PrismQueryEngine::execute()`

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use crate::prism_query::{ObjectMeta, PrismQuery, PrismQueryEngine, QueryResult};

// ── Index Configuration ───────────────────────────────────────────────────────

/// Maximum hot-index entries (64K). Each ObjectMeta is ~128 bytes → 8 MB total.
pub const HOT_INDEX_CAPACITY: usize = 8192; // 8K for embedded (bump to 65536 on real hardware)

// ── Registration Source ───────────────────────────────────────────────────────

/// Who registered this object in the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RegisteredBy {
    SiloSpawn     = 0,
    GhostWrite    = 1,
    PrismCache    = 2,
    Manual        = 3,
}

// ── Index Statistics ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct LiveIndexStats {
    pub total_registered: u64,
    pub evictions: u64,
    pub query_executions: u64,
    pub query_results_total: u64,
    pub lookups_by_oid: u64,
    pub updates: u64,
}

// ── Live Object Index ─────────────────────────────────────────────────────────

/// Hot in-kernel object metadata index.
/// Ring buffer — evicts oldest on overflow.
pub struct LiveObjectIndex {
    /// Circular buffer of object metadata
    entries: Vec<ObjectMeta>,
    /// Write head — next slot to write (wraps)
    head: usize,
    /// Total capacity
    capacity: usize,
    /// True if the ring has wrapped at least once (fully populated)
    full: bool,
    /// Q-Engine for running queries against this index
    engine: PrismQueryEngine,
    pub stats: LiveIndexStats,
}

impl LiveObjectIndex {
    pub fn new() -> Self {
        LiveObjectIndex {
            entries: Vec::new(),
            head: 0,
            capacity: HOT_INDEX_CAPACITY,
            full: false,
            engine: PrismQueryEngine::new(),
            stats: LiveIndexStats::default(),
        }
    }

    /// Register an object in the index. Evicts oldest if full.
    pub fn register(
        &mut self,
        oid: [u8; 32],
        obj_type: &str,
        size_bytes: u64,
        creator_silo: u64,
        created_at: u64,
        tags: Vec<String>,
        source: RegisteredBy,
    ) {
        let meta = ObjectMeta {
            oid,
            object_type: obj_type.into(),
            size_bytes,
            creator_silo,
            created_tick: created_at,
            modified_tick: created_at,
            tags,
            content_hash: oid, // content-addressed: OID = hash
            uns_uri: alloc::format!("prism://{:02x}{:02x}..", oid[0], oid[1]),
            text_snippet: None,
            linked_oids: alloc::vec![],
        };

        self.stats.total_registered += 1;

        if self.entries.len() < self.capacity {
            self.entries.push(meta);
            self.head = self.entries.len() % self.capacity;
        } else {
            // Ring full — evict the entry at `head`
            self.entries[self.head] = meta;
            self.head = (self.head + 1) % self.capacity;
            self.full = true;
            self.stats.evictions += 1;
        }

        crate::serial_println!(
            "[PRISM INDEX] Registered OID {:02x}{:02x}.. type={} size={}B src={:?}",
            oid[0], oid[1], obj_type, size_bytes, source
        );
    }

    /// Update an existing entry by OID (on Ghost-Write version bump).
    pub fn update_version(&mut self, oid: &[u8; 32], new_size: u64, tick: u64) {
        for entry in self.entries.iter_mut() {
            if &entry.oid == oid {
                entry.size_bytes = new_size;
                entry.modified_tick = tick;
                self.stats.updates += 1;
                return;
            }
        }
    }

    /// Evict all objects owned by a Silo (on vaporize).
    pub fn evict_silo(&mut self, silo_id: u64) {
        self.entries.retain(|e| e.creator_silo != silo_id);
        // Recalculate head after removal (simplest: reset to len)
        self.head = self.entries.len() % self.capacity.max(1);
    }

    /// Return metadata for an OID if in the hot index.
    pub fn lookup(&mut self, oid: &[u8; 32]) -> Option<&ObjectMeta> {
        self.stats.lookups_by_oid += 1;
        self.entries.iter().find(|e| &e.oid == oid)
    }

    /// Execute a structured PrismQuery against the live index.
    /// Returns matched + sorted results.
    pub fn execute_query(&mut self, query: &PrismQuery) -> Vec<QueryResult> {
        self.stats.query_executions += 1;
        let results = self.engine.execute(query, &self.entries);
        self.stats.query_results_total += results.len() as u64;
        crate::serial_println!(
            "[PRISM INDEX] Query executed — matched {} / {} entries",
            results.len(), self.entries.len()
        );
        results
    }

    /// Snapshot the current index size.
    pub fn entry_count(&self) -> usize { self.entries.len() }

    /// Register a new Silo binary in the index (called from boot_sequence::on_silo_ready).
    pub fn register_silo_binary(&mut self, silo_id: u64, binary_oid: [u8; 32], tick: u64) {
        self.register(
            binary_oid,
            "binary",
            0, // size unknown at registration
            silo_id,
            tick,
            alloc::vec!["silo".into(), "binary".into()],
            RegisteredBy::SiloSpawn,
        );
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  LiveObjectIndex: entries={}/{} registered={} evictions={} queries={} results={}",
            self.entries.len(), self.capacity,
            self.stats.total_registered, self.stats.evictions,
            self.stats.query_executions, self.stats.query_results_total
        );
    }
}
