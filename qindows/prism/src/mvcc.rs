//! # Prism MVCC (Multi-Version Concurrency Control)
//!
//! Provides snapshot isolation for concurrent reads and writes
//! to the Prism object graph. Each transaction sees a consistent
//! snapshot, and conflicts are detected at commit time.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// A version of an object.
#[derive(Debug, Clone)]
pub struct Version {
    /// Object ID
    pub oid: u64,
    /// Transaction that created this version
    pub created_by: u64,
    /// Transaction that deleted this version (None = still alive)
    pub deleted_by: Option<u64>,
    /// Timestamp of creation
    pub created_at: u64,
    /// The data
    pub data: Vec<u8>,
}

/// Transaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Active,
    Committed,
    Aborted,
}

/// A snapshot transaction.
#[derive(Debug, Clone)]
pub struct Transaction {
    /// Transaction ID
    pub txn_id: u64,
    /// Snapshot timestamp (sees all versions created before this)
    pub snapshot_ts: u64,
    /// State
    pub state: TxnState,
    /// Objects read during this transaction
    pub read_set: Vec<u64>,
    /// Objects written during this transaction
    pub write_set: Vec<u64>,
    /// Start time
    pub started_at: u64,
}

/// MVCC manager.
pub struct MvccManager {
    /// Version store: OID → list of versions (newest first)
    pub versions: BTreeMap<u64, Vec<Version>>,
    /// Active transactions
    pub transactions: BTreeMap<u64, Transaction>,
    /// Next transaction ID
    next_txn_id: u64,
    /// Global timestamp counter
    global_ts: u64,
    /// Stats
    pub stats: MvccStats,
}

/// MVCC statistics.
#[derive(Debug, Clone, Default)]
pub struct MvccStats {
    pub txns_started: u64,
    pub txns_committed: u64,
    pub txns_aborted: u64,
    pub conflicts_detected: u64,
    pub versions_created: u64,
    pub versions_garbage_collected: u64,
}

impl MvccManager {
    pub fn new() -> Self {
        MvccManager {
            versions: BTreeMap::new(),
            transactions: BTreeMap::new(),
            next_txn_id: 1,
            global_ts: 0,
            stats: MvccStats::default(),
        }
    }

    /// Begin a new snapshot transaction.
    pub fn begin(&mut self) -> u64 {
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.global_ts += 1;

        let txn = Transaction {
            txn_id,
            snapshot_ts: self.global_ts,
            state: TxnState::Active,
            read_set: Vec::new(),
            write_set: Vec::new(),
            started_at: self.global_ts,
        };

        self.transactions.insert(txn_id, txn);
        self.stats.txns_started += 1;
        txn_id
    }

    /// Read an object (returns the version visible to this transaction's snapshot).
    pub fn read(&mut self, txn_id: u64, oid: u64) -> Option<Vec<u8>> {
        let snapshot_ts = self.transactions.get(&txn_id)?.snapshot_ts;

        // Record in read set
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            if !txn.read_set.contains(&oid) {
                txn.read_set.push(oid);
            }
        }

        // Find the latest version visible to this snapshot
        let versions = self.versions.get(&oid)?;
        for ver in versions {
            // Version must be created before our snapshot
            if ver.created_at > snapshot_ts { continue; }

            // Check if the creating transaction committed
            if let Some(creator_txn) = self.transactions.get(&ver.created_by) {
                if creator_txn.state != TxnState::Committed && ver.created_by != txn_id {
                    continue; // Uncommitted version from another txn
                }
            }

            // Check if not deleted (or deleted after our snapshot)
            if let Some(del_txn) = ver.deleted_by {
                if let Some(deleter) = self.transactions.get(&del_txn) {
                    if deleter.state == TxnState::Committed && deleter.snapshot_ts <= snapshot_ts {
                        continue; // Deleted before our snapshot
                    }
                }
            }

            return Some(ver.data.clone());
        }

        None
    }

    /// Write an object (creates a new version).
    pub fn write(&mut self, txn_id: u64, oid: u64, data: Vec<u8>) -> bool {
        let ts = match self.transactions.get(&txn_id) {
            Some(txn) if txn.state == TxnState::Active => txn.snapshot_ts,
            _ => return false,
        };

        let version = Version {
            oid,
            created_by: txn_id,
            deleted_by: None,
            created_at: ts,
            data,
        };

        self.versions.entry(oid).or_insert_with(Vec::new).insert(0, version);
        self.stats.versions_created += 1;

        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            if !txn.write_set.contains(&oid) {
                txn.write_set.push(oid);
            }
        }

        true
    }

    /// Delete an object (marks current version as deleted).
    pub fn delete(&mut self, txn_id: u64, oid: u64) -> bool {
        if let Some(versions) = self.versions.get_mut(&oid) {
            for ver in versions.iter_mut() {
                if ver.deleted_by.is_none() {
                    ver.deleted_by = Some(txn_id);
                    break;
                }
            }
        }

        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            if !txn.write_set.contains(&oid) {
                txn.write_set.push(oid);
            }
        }

        true
    }

    /// Commit a transaction (with conflict detection).
    pub fn commit(&mut self, txn_id: u64) -> Result<(), &'static str> {
        let txn = match self.transactions.get(&txn_id) {
            Some(t) if t.state == TxnState::Active => t.clone(),
            _ => return Err("Transaction not active"),
        };

        // Check for write-write conflicts
        for &oid in &txn.write_set {
            if let Some(versions) = self.versions.get(&oid) {
                for ver in versions {
                    // Another committed transaction wrote to this OID after our snapshot
                    if ver.created_by != txn_id
                        && ver.created_at > txn.snapshot_ts
                    {
                        if let Some(other_txn) = self.transactions.get(&ver.created_by) {
                            if other_txn.state == TxnState::Committed {
                                self.stats.conflicts_detected += 1;
                                // Abort this transaction
                                if let Some(t) = self.transactions.get_mut(&txn_id) {
                                    t.state = TxnState::Aborted;
                                }
                                self.stats.txns_aborted += 1;
                                return Err("Write-write conflict");
                            }
                        }
                    }
                }
            }
        }

        // No conflicts — commit
        self.global_ts += 1;
        if let Some(t) = self.transactions.get_mut(&txn_id) {
            t.state = TxnState::Committed;
        }
        self.stats.txns_committed += 1;

        Ok(())
    }

    /// Abort a transaction.
    pub fn abort(&mut self, txn_id: u64) {
        if let Some(txn) = self.transactions.get_mut(&txn_id) {
            txn.state = TxnState::Aborted;
            self.stats.txns_aborted += 1;
        }

        // Remove versions created by this transaction
        for versions in self.versions.values_mut() {
            versions.retain(|v| v.created_by != txn_id);
        }
    }

    /// Garbage-collect old versions no longer visible to any active transaction.
    pub fn gc(&mut self) -> usize {
        let min_active_ts = self.transactions.values()
            .filter(|t| t.state == TxnState::Active)
            .map(|t| t.snapshot_ts)
            .min()
            .unwrap_or(self.global_ts);

        let mut collected = 0;

        for versions in self.versions.values_mut() {
            let before = versions.len();
            versions.retain(|v| {
                // Keep if created after min_active or is the latest version
                v.created_at >= min_active_ts || v.deleted_by.is_none()
            });
            collected += before - versions.len();
        }

        self.stats.versions_garbage_collected += collected as u64;
        collected
    }
}
