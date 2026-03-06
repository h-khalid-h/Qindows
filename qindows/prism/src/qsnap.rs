//! # Q-Snap — Atomic Filesystem Snapshots via COW
//!
//! Creates point-in-time snapshots of the Q-Object tree
//! using copy-on-write (Section 3.17).
//!
//! Features:
//! - Instant snapshot creation (O(1) via metadata clone)
//! - COW: blocks shared until modified
//! - Per-Silo snapshots (independent snapshot trees)
//! - Snapshot rollback (restore to a snapshot point)
//! - Space-efficient: only stores changed blocks

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Snapshot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapState {
    Active,
    Restoring,
    Deleted,
}

/// A filesystem snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: u64,
    pub silo_id: u64,
    pub name: String,
    pub state: SnapState,
    pub parent_id: Option<u64>,
    pub created_at: u64,
    pub block_count: u64,
    pub cow_blocks: u64,
}

/// Snapshot statistics.
#[derive(Debug, Clone, Default)]
pub struct SnapStats {
    pub snapshots_created: u64,
    pub snapshots_deleted: u64,
    pub snapshots_restored: u64,
    pub cow_copies: u64,
    pub blocks_shared: u64,
}

/// The Q-Snap Manager.
pub struct QSnap {
    pub snapshots: BTreeMap<u64, Snapshot>,
    /// Silo → list of snapshot IDs (newest last)
    pub silo_snaps: BTreeMap<u64, Vec<u64>>,
    pub max_snaps_per_silo: usize,
    next_id: u64,
    pub stats: SnapStats,
}

impl QSnap {
    pub fn new() -> Self {
        QSnap {
            snapshots: BTreeMap::new(),
            silo_snaps: BTreeMap::new(),
            max_snaps_per_silo: 32,
            next_id: 1,
            stats: SnapStats::default(),
        }
    }

    /// Create a snapshot.
    pub fn create(&mut self, silo_id: u64, name: &str, block_count: u64, now: u64) -> Result<u64, &'static str> {
        let silo_list = self.silo_snaps.entry(silo_id).or_insert_with(Vec::new);
        if silo_list.len() >= self.max_snaps_per_silo {
            return Err("Snapshot limit reached");
        }

        let id = self.next_id;
        self.next_id += 1;
        let parent = silo_list.last().copied();

        self.snapshots.insert(id, Snapshot {
            id, silo_id, name: String::from(name),
            state: SnapState::Active, parent_id: parent,
            created_at: now, block_count, cow_blocks: 0,
        });
        silo_list.push(id);

        self.stats.snapshots_created += 1;
        self.stats.blocks_shared += block_count;
        Ok(id)
    }

    /// Record a COW copy (block modified after snapshot).
    pub fn cow_copy(&mut self, snap_id: u64) {
        if let Some(snap) = self.snapshots.get_mut(&snap_id) {
            snap.cow_blocks += 1;
            self.stats.cow_copies += 1;
        }
    }

    /// Delete a snapshot.
    pub fn delete(&mut self, snap_id: u64) {
        if let Some(snap) = self.snapshots.get_mut(&snap_id) {
            snap.state = SnapState::Deleted;
            let silo_id = snap.silo_id;
            // Remove from silo list
            if let Some(silo_list) = self.silo_snaps.get_mut(&silo_id) {
                silo_list.retain(|&id| id != snap_id);
            }
            self.stats.snapshots_deleted += 1;
        }
    }

    /// Restore to a snapshot.
    pub fn restore(&mut self, snap_id: u64) -> Result<u64, &'static str> {
        let snap = self.snapshots.get_mut(&snap_id).ok_or("Snapshot not found")?;
        if snap.state != SnapState::Active {
            return Err("Snapshot not active");
        }
        snap.state = SnapState::Restoring;
        self.stats.snapshots_restored += 1;
        Ok(snap.block_count)
    }

    /// List snapshots for a Silo.
    pub fn list(&self, silo_id: u64) -> Vec<&Snapshot> {
        self.silo_snaps.get(&silo_id)
            .map(|ids| ids.iter().filter_map(|id| self.snapshots.get(id)).collect())
            .unwrap_or_default()
    }
}
