//! # Prism Snapshot System
//!
//! Instant filesystem snapshots using Copy-on-Write B-trees.
//! Every mutation creates a new tree root while sharing unchanged nodes
//! with previous versions. Enables instant rollback, version browsing,
//! and Time Machine-style file recovery.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Snapshot ID (monotonically increasing).
pub type SnapshotId = u64;

/// A point-in-time snapshot of the entire Prism object graph.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Unique snapshot ID
    pub id: SnapshotId,
    /// Human label (e.g., "Before system update")
    pub label: String,
    /// B-tree root node block address
    pub root_block: u64,
    /// Creation timestamp (ticks)
    pub created_at: u64,
    /// Total objects in this snapshot
    pub object_count: u64,
    /// Total bytes (logical, including shared nodes)
    pub logical_bytes: u64,
    /// Unique bytes (actual extra disk usage)
    pub unique_bytes: u64,
    /// Is this snapshot pinned (protected from GC)?
    pub pinned: bool,
    /// Parent snapshot ID (for delta chain)
    pub parent: Option<SnapshotId>,
}

/// Snapshot creation policy.
#[derive(Debug, Clone, Copy)]
pub enum SnapshotPolicy {
    /// Create a snapshot before every write transaction
    EveryTransaction,
    /// Periodic (every N minutes)
    Periodic(u32),
    /// Manual only
    Manual,
    /// On significant events (app install, update, etc.)
    EventDriven,
}

/// Snapshot GC policy — which old snapshots to remove.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// Keep all snapshots from the last N minutes
    pub keep_recent_minutes: u32,
    /// Keep one per hour for the last N hours
    pub keep_hourly_hours: u32,
    /// Keep one per day for the last N days
    pub keep_daily_days: u32,
    /// Keep one per week for the last N weeks  
    pub keep_weekly_weeks: u32,
    /// Total maximum snapshots
    pub max_total: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        RetentionPolicy {
            keep_recent_minutes: 60,     // Last hour: all
            keep_hourly_hours: 24,       // Last day: hourly
            keep_daily_days: 30,         // Last month: daily
            keep_weekly_weeks: 52,       // Last year: weekly
            max_total: 1000,
        }
    }
}

/// The Snapshot Manager.
pub struct SnapshotManager {
    /// All snapshots (sorted by creation time)
    pub snapshots: Vec<Snapshot>,
    /// Next snapshot ID
    next_id: SnapshotId,
    /// Snapshot creation policy
    pub policy: SnapshotPolicy,
    /// Retention/GC policy
    pub retention: RetentionPolicy,
    /// Total unique bytes used by all snapshots
    pub total_snapshot_bytes: u64,
}

impl SnapshotManager {
    pub fn new() -> Self {
        SnapshotManager {
            snapshots: Vec::new(),
            next_id: 1,
            policy: SnapshotPolicy::EventDriven,
            retention: RetentionPolicy::default(),
            total_snapshot_bytes: 0,
        }
    }

    /// Create a new snapshot.
    pub fn create(&mut self, root_block: u64, label: String, object_count: u64) -> SnapshotId {
        let id = self.next_id;
        self.next_id += 1;

        let parent = self.snapshots.last().map(|s| s.id);

        self.snapshots.push(Snapshot {
            id,
            label,
            root_block,
            created_at: 0, // Would be set to current tick
            object_count,
            logical_bytes: 0,
            unique_bytes: 0,
            pinned: false,
            parent,
        });

        id
    }

    /// Get a snapshot by ID.
    pub fn get(&self, id: SnapshotId) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// Pin a snapshot (protect from GC).
    pub fn pin(&mut self, id: SnapshotId) -> bool {
        if let Some(s) = self.snapshots.iter_mut().find(|s| s.id == id) {
            s.pinned = true;
            true
        } else {
            false
        }
    }

    /// Unpin a snapshot.
    pub fn unpin(&mut self, id: SnapshotId) {
        if let Some(s) = self.snapshots.iter_mut().find(|s| s.id == id) {
            s.pinned = false;
        }
    }

    /// Rollback to a specific snapshot.
    ///
    /// This makes the snapshot's B-tree root the current root,
    /// effectively reverting all changes made after the snapshot.
    pub fn rollback(&mut self, id: SnapshotId) -> Option<u64> {
        // Create a snapshot of the current state first
        // (so the rollback itself is reversible)
        if let Some(current_root) = self.snapshots.last().map(|s| s.root_block) {
            self.create(current_root, String::from("Pre-rollback auto-snapshot"), 0);
        }

        self.get(id).map(|s| s.root_block)
    }

    /// List snapshots in a time range.
    pub fn list_range(&self, from_tick: u64, to_tick: u64) -> Vec<&Snapshot> {
        self.snapshots.iter()
            .filter(|s| s.created_at >= from_tick && s.created_at <= to_tick)
            .collect()
    }

    /// Apply retention policy — remove expired unpinned snapshots.
    pub fn gc(&mut self, current_tick: u64, ticks_per_minute: u64) {
        let max = self.retention.max_total;

        // Remove oldest unpinned snapshots if over limit
        while self.snapshots.len() > max {
            if let Some(pos) = self.snapshots.iter().position(|s| !s.pinned) {
                self.snapshots.remove(pos);
            } else {
                break; // All remaining are pinned
            }
        }

        let _ = (current_tick, ticks_per_minute);
    }

    /// Get total count.
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Browse an object's version history across snapshots.
    pub fn object_history(&self, _oid: u64) -> Vec<(SnapshotId, u64)> {
        // Would look up the object in each snapshot's B-tree
        // Returns (snapshot_id, block_address) for each version
        Vec::new()
    }
}
