//! # Q-Trash — Soft-Delete with Retention & Recovery
//!
//! Deleted Q-Objects go to a per-Silo trash bin with configurable
//! retention before permanent deletion (Section 3.9).
//!
//! Features:
//! - Soft-delete preserves object until retention expires
//! - Per-Silo trash isolation
//! - Restore to original location
//! - Auto-purge based on age or total trash size
//! - Admin override for immediate purge

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A trashed item.
#[derive(Debug, Clone)]
pub struct TrashItem {
    pub id: u64,
    pub oid: u64,
    pub silo_id: u64,
    pub original_path: String,
    pub size: u64,
    pub trashed_at: u64,
    pub expires_at: u64,
    pub restored: bool,
    pub purged: bool,
}

/// Trash statistics.
#[derive(Debug, Clone, Default)]
pub struct TrashStats {
    pub items_trashed: u64,
    pub items_restored: u64,
    pub items_purged: u64,
    pub bytes_in_trash: u64,
    pub bytes_purged: u64,
}

/// The Q-Trash Manager.
pub struct QTrash {
    /// Per-Silo trash bins
    pub bins: BTreeMap<u64, Vec<TrashItem>>,
    /// Retention period (seconds)
    pub retention_secs: u64,
    /// Max trash size per Silo (bytes)
    pub max_size_per_silo: u64,
    next_id: u64,
    pub stats: TrashStats,
}

impl QTrash {
    pub fn new() -> Self {
        QTrash {
            bins: BTreeMap::new(),
            retention_secs: 86400 * 30,  // 30 days
            max_size_per_silo: 10 * 1024 * 1024 * 1024, // 10 GB
            next_id: 1,
            stats: TrashStats::default(),
        }
    }

    /// Soft-delete a Q-Object.
    pub fn trash(&mut self, oid: u64, silo_id: u64, path: &str, size: u64, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let bin = self.bins.entry(silo_id).or_insert_with(Vec::new);
        bin.push(TrashItem {
            id, oid, silo_id, original_path: String::from(path),
            size, trashed_at: now, expires_at: now + self.retention_secs,
            restored: false, purged: false,
        });

        self.stats.items_trashed += 1;
        self.stats.bytes_in_trash += size;

        // Auto-purge if over size limit
        self.enforce_size_limit(silo_id);

        id
    }

    /// Restore a trashed item.
    pub fn restore(&mut self, silo_id: u64, trash_id: u64) -> Result<u64, &'static str> {
        let bin = self.bins.get_mut(&silo_id).ok_or("No trash bin")?;
        let item = bin.iter_mut()
            .find(|i| i.id == trash_id && !i.restored && !i.purged)
            .ok_or("Item not found or already processed")?;

        item.restored = true;
        self.stats.items_restored += 1;
        self.stats.bytes_in_trash = self.stats.bytes_in_trash.saturating_sub(item.size);
        Ok(item.oid)
    }

    /// Purge expired items.
    pub fn purge_expired(&mut self, now: u64) {
        for bin in self.bins.values_mut() {
            for item in bin.iter_mut() {
                if !item.purged && !item.restored && now >= item.expires_at {
                    item.purged = true;
                    self.stats.items_purged += 1;
                    self.stats.bytes_purged += item.size;
                    self.stats.bytes_in_trash = self.stats.bytes_in_trash.saturating_sub(item.size);
                }
            }
        }
    }

    /// Enforce per-Silo size limit (purge oldest first).
    fn enforce_size_limit(&mut self, silo_id: u64) {
        if let Some(bin) = self.bins.get_mut(&silo_id) {
            let total: u64 = bin.iter()
                .filter(|i| !i.purged && !i.restored)
                .map(|i| i.size)
                .sum();

            if total <= self.max_size_per_silo {
                return;
            }

            let mut excess = total - self.max_size_per_silo;
            for item in bin.iter_mut() {
                if excess == 0 { break; }
                if !item.purged && !item.restored {
                    item.purged = true;
                    self.stats.items_purged += 1;
                    self.stats.bytes_purged += item.size;
                    self.stats.bytes_in_trash = self.stats.bytes_in_trash.saturating_sub(item.size);
                    excess = excess.saturating_sub(item.size);
                }
            }
        }
    }

    /// Immediate admin purge.
    pub fn admin_purge(&mut self, silo_id: u64) {
        if let Some(bin) = self.bins.get_mut(&silo_id) {
            for item in bin.iter_mut() {
                if !item.purged && !item.restored {
                    item.purged = true;
                    self.stats.items_purged += 1;
                    self.stats.bytes_purged += item.size;
                    self.stats.bytes_in_trash = self.stats.bytes_in_trash.saturating_sub(item.size);
                }
            }
        }
    }
}
