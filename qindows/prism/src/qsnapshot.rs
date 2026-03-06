//! # Q-Snapshot — Volume-Level COW Snapshots
//!
//! Provides copy-on-write snapshots of the Q-Object tree
//! for instant backup and time-travel queries (Section 3.25).
//!
//! Features:
//! - Instant COW snapshots
//! - Per-Silo snapshot limits
//! - Block-level change tracking
//! - Snapshot diff computation
//! - Space-efficient shared blocks

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Snapshot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapState {
    Active,
    Frozen,
    Deleting,
}

/// A COW snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: u64,
    pub silo_id: u64,
    pub parent_id: Option<u64>,
    pub state: SnapState,
    pub created_at: u64,
    /// Changed blocks (block_id → new data offset)
    pub cow_blocks: BTreeMap<u64, u64>,
    pub total_blocks: u64,
    pub cow_block_count: u64,
}

/// Snapshot statistics.
#[derive(Debug, Clone, Default)]
pub struct SnapshotStats {
    pub snapshots_created: u64,
    pub snapshots_deleted: u64,
    pub cow_writes: u64,
    pub blocks_shared: u64,
}

/// The Q-Snapshot Manager.
pub struct QSnapshot {
    pub snapshots: BTreeMap<u64, Snapshot>,
    next_id: u64,
    pub max_per_silo: usize,
    pub stats: SnapshotStats,
}

impl QSnapshot {
    pub fn new() -> Self {
        QSnapshot {
            snapshots: BTreeMap::new(),
            next_id: 1,
            max_per_silo: 256,
            stats: SnapshotStats::default(),
        }
    }

    /// Create a snapshot. Returns snapshot ID.
    pub fn create(&mut self, silo_id: u64, parent: Option<u64>, total_blocks: u64, now: u64) -> Result<u64, &'static str> {
        // Check Silo limit
        let count = self.snapshots.values()
            .filter(|s| s.silo_id == silo_id && s.state != SnapState::Deleting)
            .count();
        if count >= self.max_per_silo {
            return Err("Snapshot limit reached");
        }

        let id = self.next_id;
        self.next_id += 1;

        self.snapshots.insert(id, Snapshot {
            id, silo_id, parent_id: parent, state: SnapState::Frozen,
            created_at: now, cow_blocks: BTreeMap::new(),
            total_blocks, cow_block_count: 0,
        });

        self.stats.snapshots_created += 1;
        Ok(id)
    }

    /// Record a COW write (block was modified after snapshot).
    pub fn cow_write(&mut self, snapshot_id: u64, block_id: u64, new_offset: u64) {
        if let Some(snap) = self.snapshots.get_mut(&snapshot_id) {
            if !snap.cow_blocks.contains_key(&block_id) {
                snap.cow_blocks.insert(block_id, new_offset);
                snap.cow_block_count += 1;
                self.stats.cow_writes += 1;
            }
        }
    }

    /// Read a block — check snapshot first, fall back to parent.
    pub fn read_block(&self, snapshot_id: u64, block_id: u64) -> Option<u64> {
        let snap = self.snapshots.get(&snapshot_id)?;
        if let Some(&offset) = snap.cow_blocks.get(&block_id) {
            return Some(offset);
        }
        // Check parent chain
        if let Some(parent_id) = snap.parent_id {
            return self.read_block(parent_id, block_id);
        }
        None // Block not in snapshot chain
    }

    /// Compute diff between two snapshots (blocks changed).
    pub fn diff(&self, snap_a: u64, snap_b: u64) -> Vec<u64> {
        let a_blocks: Vec<u64> = self.snapshots.get(&snap_a)
            .map(|s| s.cow_blocks.keys().copied().collect())
            .unwrap_or_default();
        let b_blocks: Vec<u64> = self.snapshots.get(&snap_b)
            .map(|s| s.cow_blocks.keys().copied().collect())
            .unwrap_or_default();

        // Symmetric difference
        let mut diff = Vec::new();
        for &b in &a_blocks { if !b_blocks.contains(&b) { diff.push(b); } }
        for &b in &b_blocks { if !a_blocks.contains(&b) { diff.push(b); } }
        diff
    }

    /// Delete a snapshot.
    pub fn delete(&mut self, snapshot_id: u64) {
        if let Some(snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = SnapState::Deleting;
            self.stats.snapshots_deleted += 1;
        }
    }
}
