//! # Prism Change Log
//!
//! Tracks all mutations to the Prism object graph.
//! Provides an audit trail, enables undo/redo, and powers
//! real-time sync across devices via Nexus.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Types of changes tracked.
#[derive(Debug, Clone)]
pub enum ChangeType {
    /// Object created
    Create { oid: u64, size: u64 },
    /// Object modified
    Modify { oid: u64, offset: u64, old_size: u64, new_size: u64 },
    /// Object deleted
    Delete { oid: u64, size: u64 },
    /// Object renamed
    Rename { oid: u64, old_name: String, new_name: String },
    /// Object moved (different parent)
    Move { oid: u64, old_parent: u64, new_parent: u64 },
    /// Metadata changed (permissions, timestamps)
    MetadataChange { oid: u64, key: String, old_value: String, new_value: String },
    /// Snapshot created
    SnapshotCreated { snapshot_id: u64 },
    /// Snapshot deleted
    SnapshotDeleted { snapshot_id: u64 },
}

/// A single change log entry.
#[derive(Debug, Clone)]
pub struct ChangeEntry {
    /// Unique change ID (monotonically increasing)
    pub id: u64,
    /// Change type
    pub change: ChangeType,
    /// Silo that made the change
    pub silo_id: u64,
    /// Timestamp (ns since boot)
    pub timestamp: u64,
    /// Is this change part of a transaction?
    pub transaction_id: Option<u64>,
    /// Has this been synced to other devices?
    pub synced: bool,
}

/// A transaction (group of atomic changes).
#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: u64,
    pub changes: Vec<u64>, // Change IDs
    pub committed: bool,
}

/// The Change Log.
pub struct ChangeLog {
    /// All entries (newest at end)
    pub entries: Vec<ChangeEntry>,
    /// Active transactions
    pub transactions: Vec<Transaction>,
    /// Next change ID
    next_id: u64,
    /// Next transaction ID
    next_txn_id: u64,
    /// Max entries before compaction
    pub max_entries: usize,
    /// Sync cursor (entries before this have been synced)
    pub sync_cursor: u64,
    /// Stats
    pub stats: ChangeStats,
}

/// Change log statistics.
#[derive(Debug, Clone, Default)]
pub struct ChangeStats {
    pub total_creates: u64,
    pub total_modifies: u64,
    pub total_deletes: u64,
    pub total_renames: u64,
    pub total_moves: u64,
    pub transactions_committed: u64,
    pub transactions_rolled_back: u64,
}

impl ChangeLog {
    pub fn new() -> Self {
        ChangeLog {
            entries: Vec::new(),
            transactions: Vec::new(),
            next_id: 1,
            next_txn_id: 1,
            max_entries: 10_000,
            sync_cursor: 0,
            stats: ChangeStats::default(),
        }
    }

    /// Record a change.
    pub fn record(&mut self, change: ChangeType, silo_id: u64, txn_id: Option<u64>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        // Update stats
        match &change {
            ChangeType::Create { .. } => self.stats.total_creates += 1,
            ChangeType::Modify { .. } => self.stats.total_modifies += 1,
            ChangeType::Delete { .. } => self.stats.total_deletes += 1,
            ChangeType::Rename { .. } => self.stats.total_renames += 1,
            ChangeType::Move { .. } => self.stats.total_moves += 1,
            _ => {}
        }

        let entry = ChangeEntry {
            id,
            change,
            silo_id,
            timestamp: 0,
            transaction_id: txn_id,
            synced: false,
        };

        // Add to transaction if applicable
        if let Some(tid) = txn_id {
            if let Some(txn) = self.transactions.iter_mut().find(|t| t.id == tid) {
                txn.changes.push(id);
            }
        }

        self.entries.push(entry);

        // Compact if over limit
        if self.entries.len() > self.max_entries {
            self.compact();
        }

        id
    }

    /// Begin a transaction.
    pub fn begin_transaction(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        self.transactions.push(Transaction {
            id,
            changes: Vec::new(),
            committed: false,
        });
        id
    }

    /// Commit a transaction.
    pub fn commit(&mut self, txn_id: u64) {
        if let Some(txn) = self.transactions.iter_mut().find(|t| t.id == txn_id) {
            txn.committed = true;
            self.stats.transactions_committed += 1;
        }
    }

    /// Rollback a transaction (remove its changes).
    pub fn rollback(&mut self, txn_id: u64) {
        if let Some(txn) = self.transactions.iter().find(|t| t.id == txn_id) {
            let change_ids: Vec<u64> = txn.changes.clone();
            self.entries.retain(|e| !change_ids.contains(&e.id));
            self.stats.transactions_rolled_back += 1;
        }
        self.transactions.retain(|t| t.id != txn_id);
    }

    /// Get changes for a specific object.
    pub fn history(&self, oid: u64) -> Vec<&ChangeEntry> {
        self.entries.iter().filter(|e| {
            match &e.change {
                ChangeType::Create { oid: o, .. } => *o == oid,
                ChangeType::Modify { oid: o, .. } => *o == oid,
                ChangeType::Delete { oid: o, .. } => *o == oid,
                ChangeType::Rename { oid: o, .. } => *o == oid,
                ChangeType::Move { oid: o, .. } => *o == oid,
                ChangeType::MetadataChange { oid: o, .. } => *o == oid,
                _ => false,
            }
        }).collect()
    }

    /// Get unsynced changes (for Nexus sync).
    pub fn unsynced(&self) -> Vec<&ChangeEntry> {
        self.entries.iter().filter(|e| !e.synced && e.id > self.sync_cursor).collect()
    }

    /// Mark entries as synced.
    pub fn mark_synced(&mut self, up_to_id: u64) {
        for entry in &mut self.entries {
            if entry.id <= up_to_id {
                entry.synced = true;
            }
        }
        self.sync_cursor = up_to_id;
    }

    /// Compact old entries (keep recent ones).
    fn compact(&mut self) {
        let keep = self.max_entries / 2;
        if self.entries.len() > keep {
            let drain = self.entries.len() - keep;
            self.entries.drain(0..drain);
        }
    }

    /// Get recent changes (last N).
    pub fn recent(&self, count: usize) -> Vec<&ChangeEntry> {
        self.entries.iter().rev().take(count).collect()
    }
}
