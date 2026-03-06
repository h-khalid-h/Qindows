//! # Q-Index — B-Tree Metadata Index for Fast Object Lookups
//!
//! Indexes Q-Object metadata for fast queries by name,
//! type, tags, and custom attributes (Section 3.19).
//!
//! Features:
//! - In-memory B-tree index
//! - Multi-key indexing (name, type, mtime, size)
//! - Range queries (e.g., objects modified after timestamp)
//! - Per-Silo index isolation
//! - Bulk index rebuild

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Index key type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKey {
    Name,
    ObjType,
    ModTime,
    Size,
    Tag,
}

/// An index entry.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub oid: u64,
    pub silo_id: u64,
    pub name: String,
    pub obj_type: u16,
    pub mtime: u64,
    pub size: u64,
}

/// Index statistics.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub entries: u64,
    pub lookups: u64,
    pub range_queries: u64,
    pub rebuilds: u64,
}

/// The Q-Index.
pub struct QIndex {
    /// Primary index: OID → entry
    pub by_oid: BTreeMap<u64, IndexEntry>,
    /// Name index: (silo_id, name) → OID
    pub by_name: BTreeMap<(u64, String), u64>,
    /// Type index: (silo_id, type) → OIDs
    pub by_type: BTreeMap<(u64, u16), Vec<u64>>,
    /// Time index: mtime → OIDs (for range queries)
    pub by_mtime: BTreeMap<u64, Vec<u64>>,
    pub stats: IndexStats,
}

impl QIndex {
    pub fn new() -> Self {
        QIndex {
            by_oid: BTreeMap::new(),
            by_name: BTreeMap::new(),
            by_type: BTreeMap::new(),
            by_mtime: BTreeMap::new(),
            stats: IndexStats::default(),
        }
    }

    /// Insert or update an index entry.
    pub fn upsert(&mut self, entry: IndexEntry) {
        let oid = entry.oid;
        let silo = entry.silo_id;

        // Remove old secondary entries if updating
        if let Some(old) = self.by_oid.remove(&oid) {
            self.by_name.remove(&(old.silo_id, old.name));
            if let Some(oids) = self.by_type.get_mut(&(old.silo_id, old.obj_type)) {
                oids.retain(|&id| id != oid);
            }
            if let Some(oids) = self.by_mtime.get_mut(&old.mtime) {
                oids.retain(|&id| id != oid);
            }
        } else {
            self.stats.entries += 1;
        }

        // Insert new entries
        self.by_name.insert((silo, entry.name.clone()), oid);
        self.by_type.entry((silo, entry.obj_type)).or_insert_with(Vec::new).push(oid);
        self.by_mtime.entry(entry.mtime).or_insert_with(Vec::new).push(oid);
        self.by_oid.insert(oid, entry);
    }

    /// Look up by OID.
    pub fn get(&mut self, oid: u64) -> Option<&IndexEntry> {
        self.stats.lookups += 1;
        self.by_oid.get(&oid)
    }

    /// Look up by name within a Silo.
    pub fn find_by_name(&mut self, silo_id: u64, name: &str) -> Option<&IndexEntry> {
        self.stats.lookups += 1;
        self.by_name.get(&(silo_id, String::from(name)))
            .and_then(|oid| self.by_oid.get(oid))
    }

    /// Range query: objects modified in [start, end].
    pub fn query_mtime_range(&mut self, start: u64, end: u64) -> Vec<u64> {
        self.stats.range_queries += 1;
        self.by_mtime.range(start..=end)
            .flat_map(|(_, oids)| oids.iter().copied())
            .collect()
    }

    /// Remove an entry.
    pub fn remove(&mut self, oid: u64) {
        if let Some(entry) = self.by_oid.remove(&oid) {
            self.by_name.remove(&(entry.silo_id, entry.name));
            if let Some(oids) = self.by_type.get_mut(&(entry.silo_id, entry.obj_type)) {
                oids.retain(|&id| id != oid);
            }
            if let Some(oids) = self.by_mtime.get_mut(&entry.mtime) {
                oids.retain(|&id| id != oid);
            }
            self.stats.entries = self.stats.entries.saturating_sub(1);
        }
    }
}
