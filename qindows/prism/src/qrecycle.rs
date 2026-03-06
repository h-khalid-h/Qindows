//! # Q-Recycle — Soft-Delete Recycle Bin
//!
//! Implements a per-Silo recycle bin that retains deleted
//! objects for a configurable grace period before permanent
//! removal (Section 3.39).
//!
//! Features:
//! - Soft-delete with metadata preservation
//! - Per-Silo isolation
//! - Automatic purge after retention period
//! - Restore to original path
//! - Storage quota awareness (auto-purge if low)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A recycled item.
#[derive(Debug, Clone)]
pub struct RecycledItem {
    pub oid: u64,
    pub original_path: String,
    pub silo_id: u64,
    pub deleted_at: u64,
    pub size_bytes: u64,
    pub item_type: ItemType,
}

/// Item type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemType {
    File,
    Directory,
    Symlink,
}

/// Recycle bin statistics.
#[derive(Debug, Clone, Default)]
pub struct RecycleStats {
    pub items_recycled: u64,
    pub items_restored: u64,
    pub items_purged: u64,
    pub bytes_reclaimed: u64,
}

/// The Q-Recycle Bin Manager.
pub struct QRecycle {
    /// Recycled items (oid → item)
    pub items: BTreeMap<u64, RecycledItem>,
    pub retention_ms: u64,
    pub max_size_bytes: u64,
    pub current_size: u64,
    pub stats: RecycleStats,
}

impl QRecycle {
    pub fn new(retention_ms: u64, max_size: u64) -> Self {
        QRecycle {
            items: BTreeMap::new(),
            retention_ms, max_size_bytes: max_size,
            current_size: 0,
            stats: RecycleStats::default(),
        }
    }

    /// Soft-delete an item.
    pub fn recycle(&mut self, oid: u64, path: &str, silo_id: u64, size: u64, itype: ItemType, now: u64) {
        // Auto-purge if exceeding size limit
        while self.current_size + size > self.max_size_bytes && !self.items.is_empty() {
            let oldest = self.items.keys().next().copied();
            if let Some(oldest_oid) = oldest {
                self.purge_item(oldest_oid);
            } else { break; }
        }

        self.items.insert(oid, RecycledItem {
            oid, original_path: String::from(path), silo_id,
            deleted_at: now, size_bytes: size, item_type: itype,
        });
        self.current_size += size;
        self.stats.items_recycled += 1;
    }

    /// Restore an item from the recycle bin.
    pub fn restore(&mut self, oid: u64) -> Option<RecycledItem> {
        if let Some(item) = self.items.remove(&oid) {
            self.current_size = self.current_size.saturating_sub(item.size_bytes);
            self.stats.items_restored += 1;
            Some(item)
        } else { None }
    }

    /// Permanently purge one item.
    fn purge_item(&mut self, oid: u64) {
        if let Some(item) = self.items.remove(&oid) {
            self.current_size = self.current_size.saturating_sub(item.size_bytes);
            self.stats.items_purged += 1;
            self.stats.bytes_reclaimed += item.size_bytes;
        }
    }

    /// Purge all expired items.
    pub fn purge_expired(&mut self, now: u64) {
        let expired: Vec<u64> = self.items.values()
            .filter(|i| now.saturating_sub(i.deleted_at) > self.retention_ms)
            .map(|i| i.oid)
            .collect();
        for oid in expired {
            self.purge_item(oid);
        }
    }

    /// List items for a Silo.
    pub fn list_silo(&self, silo_id: u64) -> Vec<&RecycledItem> {
        self.items.values().filter(|i| i.silo_id == silo_id).collect()
    }

    /// Item count.
    pub fn count(&self) -> usize { self.items.len() }
}
