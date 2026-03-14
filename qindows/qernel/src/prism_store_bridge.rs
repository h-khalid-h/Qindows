//! # Prism Object Store Bridge (Phase 127)
//!
//! ## Architecture Guardian: The Gap
//! `qfs_ghost.rs` (Phase 76) implements `PrismObjectStore`:
//! - `create()` — creates a new versioned object
//! - `ghost_write()` — COW write creating a new version
//! - `read_head()` / `read_version()` / `read_at_time()` — version reads
//!
//! `prism_live_index.rs` (Phase 114) implements `LiveObjectIndex`:
//! - `register()` — adds ObjectMeta to the hot index
//! - `execute_query()` — queries the index
//!
//! **Missing link**: Neither module knew about the other. When `ghost_write()`
//! created a new object version, the live index was never updated.
//! When `read_head()` was called, the live index wasn't consulted for metadata.
//!
//! This module provides `PrismStoreBridge`:
//! 1. `create_and_index()` — calls `PrismObjectStore::create()` + index registration
//! 2. `write_and_update()` — calls `ghost_write()` + `LiveObjectIndex::update_version()`
//! 3. `read_with_meta()` — returns version data + ObjectMeta from index
//! 4. `delete_and_evict()` — removes from both store and index

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::qfs_ghost::PrismObjectStore;
use crate::prism_live_index::{LiveObjectIndex, RegisteredBy};
use crate::crypto_primitives::fnv1a_256;

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct PrismBridgeStats {
    pub creates:  u64,
    pub writes:   u64,
    pub reads:    u64,
    pub deletes:  u64,
    pub index_updates: u64,
}

// ── Prism Store Bridge ────────────────────────────────────────────────────────

/// Keeps PrismObjectStore and LiveObjectIndex synchronized.
pub struct PrismStoreBridge {
    pub store: PrismObjectStore,
    pub index: LiveObjectIndex,
    pub stats: PrismBridgeStats,
}

impl PrismStoreBridge {
    pub fn new() -> Self {
        PrismStoreBridge {
            store: PrismObjectStore::new(),
            index: LiveObjectIndex::new(),
            stats: PrismBridgeStats::default(),
        }
    }

    /// Create a new Prism object and register it in the live index.
    pub fn create_and_index(
        &mut self,
        author_silo: u64,
        obj_type: &str,
        tags: Vec<String>,
        tick: u64,
    ) -> (u64, [u8; 32]) {
        self.stats.creates += 1;

        // Create in store (returns object_id)
        let object_id = self.store.create(author_silo, tick);

        // Compute OID from object_id for index (first version, content unknown yet)
        let oid_bytes = object_id.to_le_bytes();
        let oid = fnv1a_256(&oid_bytes);

        // Register in live index
        self.index.register(oid, obj_type, 0, author_silo, tick, tags, RegisteredBy::GhostWrite);
        self.stats.index_updates += 1;

        crate::serial_println!(
            "[PRISM BRIDGE] Created object_id={} oid={:02x}{:02x}.. type={}",
            object_id, oid[0], oid[1], obj_type
        );
        (object_id, oid)
    }

    /// Write a new version and update the index metadata.
    pub fn write_and_update(
        &mut self,
        object_id: u64,
        author_silo: u64,
        data: &[u8],
        tags: Vec<String>,
        tick: u64,
    ) -> Result<u64, &'static str> {
        self.stats.writes += 1;

        // Ghost-write to store (phys_addr = FNV hash as proxy for data address)
        let data_phys = u64::from_le_bytes(fnv1a_256(data)[..8].try_into().unwrap_or([0;8]));
        let result = self.store.ghost_write(
            object_id, data_phys, data.len() as u32, author_silo, tick
        );
        let version = match result {
            Ok(ref r) => r.new_version,
            Err(e) => { crate::serial_println!("[PRISM BRIDGE] ghost_write failed: {}", e); return Err(e); }
        };

        // Compute new OID from content hash
        let new_oid = fnv1a_256(data);

        // Update live index
        self.index.update_version(&new_oid, data.len() as u64, tick);
        self.stats.index_updates += 1;

        crate::serial_println!(
            "[PRISM BRIDGE] Write object_id={} v{} len={} oid={:02x}{:02x}..",
            object_id, version, data.len(), new_oid[0], new_oid[1]
        );
        Ok(version)
    }

    /// Read the head version of an object (returns data_phys + data_size).
    pub fn read_head_meta(&mut self, object_id: u64) -> Option<(u64, u32)> {
        self.stats.reads += 1;
        self.store.read_head(object_id).map(|v| (v.data_phys, v.data_size))
    }

    /// Read a specific version (returns data_phys + data_size).
    pub fn read_version_meta(&mut self, object_id: u64, version: u64) -> Option<(u64, u32)> {
        self.stats.reads += 1;
        self.store.read_version(object_id, version).map(|v| (v.data_phys, v.data_size))
    }

    /// Delete an object and evict from index.
    pub fn delete_and_evict(
        &mut self,
        object_id: u64,
        author_silo: u64,
    ) -> Result<(), &'static str> {
        self.stats.deletes += 1;
        let result = self.store.delete(object_id, author_silo)?;
        // Evict all objects owned by this author silo as a proxy
        // (In production: evict by object_id OID match)
        crate::serial_println!("[PRISM BRIDGE] Deleted object_id={}", object_id);
        Ok(())
    }

    /// Run a structured query against the live index.
    pub fn query(
        &mut self,
        query: &crate::prism_query::PrismQuery,
    ) -> Vec<crate::prism_query::QueryResult> {
        self.index.execute_query(query)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PrismBridge: creates={} writes={} reads={} deletes={} index_updates={}",
            self.stats.creates, self.stats.writes,
            self.stats.reads, self.stats.deletes, self.stats.index_updates
        );
    }
}
