//! # Q-Link — Hard Links Within Q-Object Tree
//!
//! Provides hard links between Q-Objects, allowing multiple
//! names to point to the same object (Section 3.21).
//!
//! Features:
//! - Inode-style reference counting
//! - Cross-directory hard links (within same Silo)
//! - Link count tracking
//! - Orphan detection (link count → 0)
//! - Per-Silo hard link limits

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A hard link entry.
#[derive(Debug, Clone)]
pub struct HardLink {
    pub oid: u64,
    pub silo_id: u64,
    pub name: String,
    pub parent_oid: u64,
}

/// Hard link statistics.
#[derive(Debug, Clone, Default)]
pub struct LinkStats {
    pub links_created: u64,
    pub links_removed: u64,
    pub orphans_detected: u64,
}

/// The Q-Link Manager.
pub struct QLink {
    /// OID → link count
    pub link_counts: BTreeMap<u64, u32>,
    /// (parent_oid, name) → target OID
    pub links: BTreeMap<(u64, String), u64>,
    /// OID → list of (parent, name) entries
    pub reverse: BTreeMap<u64, Vec<(u64, String)>>,
    pub max_links_per_object: u32,
    pub stats: LinkStats,
}

impl QLink {
    pub fn new() -> Self {
        QLink {
            link_counts: BTreeMap::new(),
            links: BTreeMap::new(),
            reverse: BTreeMap::new(),
            max_links_per_object: 65535,
            stats: LinkStats::default(),
        }
    }

    /// Create the initial link for a new object.
    pub fn create_initial(&mut self, oid: u64, parent_oid: u64, name: &str) {
        self.link_counts.insert(oid, 1);
        let key = (parent_oid, String::from(name));
        self.links.insert(key.clone(), oid);
        self.reverse.entry(oid).or_insert_with(Vec::new).push(key);
        self.stats.links_created += 1;
    }

    /// Create a hard link (additional name for existing object).
    pub fn link(&mut self, oid: u64, parent_oid: u64, name: &str) -> Result<(), &'static str> {
        let count = self.link_counts.get(&oid).copied().ok_or("Object not found")?;
        if count >= self.max_links_per_object {
            return Err("Max hard links reached");
        }

        let key = (parent_oid, String::from(name));
        if self.links.contains_key(&key) {
            return Err("Name already exists in parent");
        }

        self.links.insert(key.clone(), oid);
        self.reverse.entry(oid).or_insert_with(Vec::new).push(key);
        *self.link_counts.get_mut(&oid).unwrap() += 1;
        self.stats.links_created += 1;
        Ok(())
    }

    /// Remove a hard link. Returns true if object is now orphaned (link count 0).
    pub fn unlink(&mut self, parent_oid: u64, name: &str) -> Option<bool> {
        let key = (parent_oid, String::from(name));
        let oid = self.links.remove(&key)?;

        if let Some(entries) = self.reverse.get_mut(&oid) {
            entries.retain(|e| e != &key);
        }

        let count = self.link_counts.get_mut(&oid)?;
        *count = count.saturating_sub(1);
        self.stats.links_removed += 1;

        if *count == 0 {
            self.stats.orphans_detected += 1;
            self.link_counts.remove(&oid);
            self.reverse.remove(&oid);
            Some(true) // Orphaned
        } else {
            Some(false)
        }
    }

    /// Get link count for an object.
    pub fn link_count(&self, oid: u64) -> u32 {
        self.link_counts.get(&oid).copied().unwrap_or(0)
    }

    /// List all names for an object.
    pub fn names(&self, oid: u64) -> Vec<&(u64, String)> {
        self.reverse.get(&oid)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}
