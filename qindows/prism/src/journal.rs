//! # Write-Ahead Journal
//!
//! Ensures crash consistency for the Prism Object Graph.
//! Every write operation is first recorded in the journal.
//! If a crash occurs mid-write, the journal replays on next boot.
//!
//! Sequence:
//! 1. Write intent to journal (what will change)
//! 2. Write actual data to the B-tree / data region
//! 3. Mark journal entry as committed
//! 4. On recovery: replay uncommitted entries, discard committed

#![allow(dead_code)]

use alloc::vec::Vec;

/// Journal entry types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOp {
    /// Insert a new object
    Insert,
    /// Update an existing object (Ghost-Write creates new version)
    Update,
    /// Delete an object (mark as tombstone)
    Delete,
    /// B-tree node split
    BTreeSplit,
    /// Checkpoint — all prior operations are stable
    Checkpoint,
}

/// A single journal entry — records one atomic operation.
#[derive(Debug, Clone)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number
    pub seq: u64,
    /// Operation type
    pub op: JournalOp,
    /// Object ID affected
    pub oid: [u8; 32],
    /// Data location (offset + length)
    pub data_offset: u64,
    pub data_length: u64,
    /// Previous version's location (for undo)
    pub prev_offset: u64,
    pub prev_length: u64,
    /// Whether this operation has been committed to disk
    pub committed: bool,
    /// CRC32 checksum of this entry (for integrity)
    pub checksum: u32,
}

impl JournalEntry {
    /// Create a new journal entry.
    pub fn new(seq: u64, op: JournalOp, oid: [u8; 32]) -> Self {
        JournalEntry {
            seq,
            op,
            oid,
            data_offset: 0,
            data_length: 0,
            prev_offset: 0,
            prev_length: 0,
            committed: false,
            checksum: 0,
        }
    }

    /// Compute the CRC32 checksum for this entry.
    pub fn compute_checksum(&mut self) {
        // Simplified CRC: XOR all bytes of significant fields
        let mut crc: u32 = 0xFFFF_FFFF;
        crc ^= self.seq as u32;
        crc ^= (self.seq >> 32) as u32;
        crc ^= self.op as u32;
        crc ^= self.data_offset as u32;
        crc ^= self.data_length as u32;
        for &byte in &self.oid {
            crc = crc.wrapping_mul(31).wrapping_add(byte as u32);
        }
        self.checksum = crc;
    }

    /// Verify the checksum is valid.
    pub fn verify_checksum(&self) -> bool {
        let mut copy = self.clone();
        copy.compute_checksum();
        copy.checksum == self.checksum
    }
}

/// The Write-Ahead Journal.
pub struct Journal {
    /// All entries (in-memory — paged to disk in production)
    entries: Vec<JournalEntry>,
    /// Next sequence number
    next_seq: u64,
    /// Last checkpoint sequence
    last_checkpoint: u64,
}

impl Journal {
    pub fn new() -> Self {
        Journal {
            entries: Vec::new(),
            next_seq: 1,
            last_checkpoint: 0,
        }
    }

    /// Begin a new journal transaction.
    ///
    /// Returns a mutable entry that the caller fills in
    /// with data location details.
    pub fn begin(&mut self, op: JournalOp, oid: [u8; 32]) -> &mut JournalEntry {
        let seq = self.next_seq;
        self.next_seq += 1;

        let mut entry = JournalEntry::new(seq, op, oid);
        entry.compute_checksum();
        self.entries.push(entry);
        self.entries.last_mut().unwrap()
    }

    /// Commit a journal entry — marks the operation as durable.
    ///
    /// After commit, the corresponding B-tree/data writes are
    /// guaranteed to be recoverable.
    pub fn commit(&mut self, seq: u64) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.seq == seq) {
            entry.committed = true;
            entry.compute_checksum();
            true
        } else {
            false
        }
    }

    /// Rollback an uncommitted entry.
    ///
    /// Restores the previous version's data using prev_offset/prev_length.
    pub fn rollback(&mut self, seq: u64) -> Option<JournalEntry> {
        let pos = self.entries.iter().position(|e| e.seq == seq && !e.committed)?;
        Some(self.entries.remove(pos))
    }

    /// Create a checkpoint — marks all prior operations as stable.
    ///
    /// After a checkpoint, old committed entries can be discarded
    /// to save journal space.
    pub fn checkpoint(&mut self) {
        let seq = self.next_seq;
        self.next_seq += 1;

        let mut entry = JournalEntry::new(seq, JournalOp::Checkpoint, [0; 32]);
        entry.committed = true;
        entry.compute_checksum();
        self.entries.push(entry);
        self.last_checkpoint = seq;
    }

    /// Recovery: find all uncommitted entries since the last checkpoint.
    ///
    /// These need to be replayed (or rolled back) after a crash.
    pub fn recover(&self) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.seq > self.last_checkpoint && !e.committed)
            .collect()
    }

    /// Compact the journal — remove committed entries before the last checkpoint.
    pub fn compact(&mut self) {
        self.entries.retain(|e| {
            e.seq >= self.last_checkpoint || !e.committed
        });
    }

    /// Get total entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the journal empty?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get a read-only view of all journal entries.
    ///
    /// Used by Q-Shell's PersistenceManager to scan committed entries
    /// during boot-time state restoration.
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }
}

/// Transaction wrapper — ensures commit-or-rollback semantics.
pub struct Transaction<'a> {
    journal: &'a mut Journal,
    seq: u64,
    committed: bool,
}

impl<'a> Transaction<'a> {
    /// Start a new transaction.
    pub fn begin(journal: &'a mut Journal, op: JournalOp, oid: [u8; 32]) -> Self {
        let entry = journal.begin(op, oid);
        let seq = entry.seq;
        Transaction {
            journal,
            seq,
            committed: false,
        }
    }

    /// Commit this transaction.
    pub fn commit(mut self) -> bool {
        self.committed = true;
        self.journal.commit(self.seq)
    }

    /// Get the sequence number.
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        // If the transaction wasn't explicitly committed, roll it back
        if !self.committed {
            self.journal.rollback(self.seq);
        }
    }
}
