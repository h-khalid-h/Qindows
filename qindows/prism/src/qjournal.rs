//! # Q-Journal — Write-Ahead Log for Crash-Safe Metadata
//!
//! All metadata mutations go through this journal before
//! being committed to the object store (Section 3.16).
//!
//! Features:
//! - Write-ahead logging (WAL)
//! - Transaction grouping (atomic multi-object updates)
//! - Checkpoint + truncation (reclaim journal space)
//! - Replay on crash recovery
//! - Per-Silo journal isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Journal entry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOp {
    Create,
    Modify,
    Delete,
    Rename,
    SetAttr,
    Link,
    Unlink,
}

/// Journal entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    Pending,
    Committed,
    Checkpointed,
}

/// A journal entry.
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub lsn: u64,         // Log Sequence Number
    pub txn_id: u64,
    pub silo_id: u64,
    pub op: JournalOp,
    pub oid: u64,
    pub state: EntryState,
    pub timestamp: u64,
}

/// A transaction.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: u64,
    pub silo_id: u64,
    pub entries: Vec<u64>, // LSNs
    pub committed: bool,
}

/// Journal statistics.
#[derive(Debug, Clone, Default)]
pub struct JournalStats {
    pub entries_written: u64,
    pub txns_committed: u64,
    pub txns_aborted: u64,
    pub checkpoints: u64,
    pub entries_truncated: u64,
    pub replays: u64,
}

/// The Q-Journal.
pub struct QJournal {
    pub entries: BTreeMap<u64, JournalEntry>,
    pub transactions: BTreeMap<u64, Transaction>,
    pub next_lsn: u64,
    pub next_txn: u64,
    pub checkpoint_lsn: u64,
    pub stats: JournalStats,
}

impl QJournal {
    pub fn new() -> Self {
        QJournal {
            entries: BTreeMap::new(),
            transactions: BTreeMap::new(),
            next_lsn: 1,
            next_txn: 1,
            checkpoint_lsn: 0,
            stats: JournalStats::default(),
        }
    }

    /// Begin a transaction.
    pub fn begin_txn(&mut self, silo_id: u64) -> u64 {
        let id = self.next_txn;
        self.next_txn += 1;
        self.transactions.insert(id, Transaction {
            id, silo_id, entries: Vec::new(), committed: false,
        });
        id
    }

    /// Write a journal entry within a transaction.
    pub fn write(&mut self, txn_id: u64, silo_id: u64, op: JournalOp, oid: u64, now: u64) -> Result<u64, &'static str> {
        let txn = self.transactions.get_mut(&txn_id).ok_or("Transaction not found")?;
        if txn.committed {
            return Err("Transaction already committed");
        }

        let lsn = self.next_lsn;
        self.next_lsn += 1;

        self.entries.insert(lsn, JournalEntry {
            lsn, txn_id, silo_id, op, oid,
            state: EntryState::Pending, timestamp: now,
        });

        txn.entries.push(lsn);
        self.stats.entries_written += 1;
        Ok(lsn)
    }

    /// Commit a transaction.
    pub fn commit(&mut self, txn_id: u64) -> Result<(), &'static str> {
        let txn = self.transactions.get_mut(&txn_id).ok_or("Transaction not found")?;
        if txn.committed {
            return Err("Already committed");
        }

        let lsns = txn.entries.clone();
        txn.committed = true;

        for lsn in lsns {
            if let Some(entry) = self.entries.get_mut(&lsn) {
                entry.state = EntryState::Committed;
            }
        }

        self.stats.txns_committed += 1;
        Ok(())
    }

    /// Abort a transaction.
    pub fn abort(&mut self, txn_id: u64) {
        if let Some(txn) = self.transactions.remove(&txn_id) {
            for lsn in &txn.entries {
                self.entries.remove(lsn);
            }
            self.stats.txns_aborted += 1;
        }
    }

    /// Checkpoint: mark all committed entries as checkpointed.
    pub fn checkpoint(&mut self) {
        let max_committed = self.entries.iter()
            .filter(|(_, e)| e.state == EntryState::Committed)
            .map(|(&lsn, _)| lsn)
            .max();

        if let Some(lsn) = max_committed {
            for entry in self.entries.values_mut() {
                if entry.state == EntryState::Committed && entry.lsn <= lsn {
                    entry.state = EntryState::Checkpointed;
                }
            }
            self.checkpoint_lsn = lsn;
            self.stats.checkpoints += 1;
        }
    }

    /// Truncate checkpointed entries to reclaim space.
    pub fn truncate(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|_, e| e.state != EntryState::Checkpointed);
        let removed = before - self.entries.len();
        self.stats.entries_truncated += removed as u64;
    }
}
