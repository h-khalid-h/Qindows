//! # Prism Write-Ahead Log (WAL)
//!
//! Crash-safe transaction logging for the Prism storage engine.
//! All mutations are written to the WAL before being applied to
//! the main data structures, enabling crash recovery.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// WAL record types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalRecordType {
    /// Begin a transaction
    TxnBegin,
    /// Write a key-value pair
    Write,
    /// Delete a key
    Delete,
    /// Commit the transaction
    TxnCommit,
    /// Abort the transaction
    TxnAbort,
    /// Checkpoint marker (safe truncation point)
    Checkpoint,
    /// Metadata update
    Meta,
}

/// A single WAL record.
#[derive(Debug, Clone)]
pub struct WalRecord {
    /// Log sequence number (monotonically increasing)
    pub lsn: u64,
    /// Record type
    pub record_type: WalRecordType,
    /// Transaction ID
    pub txn_id: u64,
    /// Key (for Write/Delete)
    pub key: Vec<u8>,
    /// Value (for Write)
    pub value: Vec<u8>,
    /// Timestamp (ns)
    pub timestamp: u64,
    /// CRC32 checksum of this record
    pub checksum: u32,
}

impl WalRecord {
    /// Compute CRC32 checksum of the record content.
    pub fn compute_checksum(&self) -> u32 {
        let mut crc: u32 = 0xFFFFFFFF;
        let lsn_bytes = self.lsn.to_le_bytes();
        let txn_bytes = self.txn_id.to_le_bytes();

        for &b in lsn_bytes.iter()
            .chain(txn_bytes.iter())
            .chain(&[self.record_type as u8])
            .chain(self.key.iter())
            .chain(self.value.iter())
        {
            let idx = ((crc ^ b as u32) & 0xFF) as usize;
            crc = CRC_TABLE[idx] ^ (crc >> 8);
        }
        crc ^ 0xFFFFFFFF
    }

    /// Verify the checksum.
    pub fn verify(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// Serialized size in bytes.
    pub fn size(&self) -> usize {
        8 + 1 + 8 + 4 + self.key.len() + 4 + self.value.len() + 8 + 4
        // lsn + type + txn_id + key_len + key + val_len + val + ts + crc
    }
}

/// An active transaction in the WAL.
#[derive(Debug, Clone)]
pub struct WalTransaction {
    pub txn_id: u64,
    pub records: Vec<u64>, // LSNs belonging to this txn
    pub started_at: u64,
    pub committed: bool,
    pub aborted: bool,
}

/// WAL statistics.
#[derive(Debug, Clone, Default)]
pub struct WalStats {
    pub records_written: u64,
    pub bytes_written: u64,
    pub txns_committed: u64,
    pub txns_aborted: u64,
    pub checkpoints: u64,
    pub recovery_replays: u64,
    pub corrupted_records: u64,
}

/// The Write-Ahead Log.
pub struct WriteAheadLog {
    /// All records (in-memory buffer; flushed to disk periodically)
    pub records: Vec<WalRecord>,
    /// Active transactions
    pub active_txns: Vec<WalTransaction>,
    /// Next LSN
    next_lsn: u64,
    /// Next transaction ID
    next_txn_id: u64,
    /// Last checkpoint LSN
    pub last_checkpoint_lsn: u64,
    /// Max buffer size before forced flush
    pub max_buffer_size: usize,
    /// Stats
    pub stats: WalStats,
}

impl WriteAheadLog {
    pub fn new() -> Self {
        WriteAheadLog {
            records: Vec::new(),
            active_txns: Vec::new(),
            next_lsn: 1,
            next_txn_id: 1,
            last_checkpoint_lsn: 0,
            max_buffer_size: 10_000,
            stats: WalStats::default(),
        }
    }

    /// Begin a new transaction.
    pub fn begin_txn(&mut self, now: u64) -> u64 {
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;

        self.write_record(WalRecordType::TxnBegin, txn_id, &[], &[], now);

        self.active_txns.push(WalTransaction {
            txn_id,
            records: Vec::new(),
            started_at: now,
            committed: false,
            aborted: false,
        });

        txn_id
    }

    /// Write a key-value pair within a transaction.
    pub fn write(&mut self, txn_id: u64, key: &[u8], value: &[u8], now: u64) -> u64 {
        let lsn = self.write_record(WalRecordType::Write, txn_id, key, value, now);
        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            txn.records.push(lsn);
        }
        lsn
    }

    /// Delete a key within a transaction.
    pub fn delete(&mut self, txn_id: u64, key: &[u8], now: u64) -> u64 {
        let lsn = self.write_record(WalRecordType::Delete, txn_id, key, &[], now);
        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            txn.records.push(lsn);
        }
        lsn
    }

    /// Commit a transaction.
    pub fn commit(&mut self, txn_id: u64, now: u64) -> bool {
        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            if txn.aborted { return false; }
            txn.committed = true;
            self.write_record(WalRecordType::TxnCommit, txn_id, &[], &[], now);
            self.stats.txns_committed += 1;
            true
        } else {
            false
        }
    }

    /// Abort a transaction.
    pub fn abort(&mut self, txn_id: u64, now: u64) {
        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            txn.aborted = true;
            self.write_record(WalRecordType::TxnAbort, txn_id, &[], &[], now);
            self.stats.txns_aborted += 1;
        }
    }

    /// Write a checkpoint marker.
    pub fn checkpoint(&mut self, now: u64) {
        let lsn = self.write_record(WalRecordType::Checkpoint, 0, &[], &[], now);
        self.last_checkpoint_lsn = lsn;
        self.stats.checkpoints += 1;

        // Truncate records before the checkpoint (only committed txns)
        self.truncate_before(lsn);
    }

    /// Recover: replay committed transactions after the last checkpoint.
    pub fn recover(&mut self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut replays = Vec::new();
        let committed_txns: Vec<u64> = self.active_txns.iter()
            .filter(|t| t.committed)
            .map(|t| t.txn_id)
            .collect();

        for record in &self.records {
            if record.lsn <= self.last_checkpoint_lsn { continue; }
            if !record.verify() {
                self.stats.corrupted_records += 1;
                continue;
            }
            if record.record_type == WalRecordType::Write
                && committed_txns.contains(&record.txn_id)
            {
                replays.push((record.key.clone(), record.value.clone()));
                self.stats.recovery_replays += 1;
            }
        }

        replays
    }

    /// Write a record to the log.
    fn write_record(
        &mut self, record_type: WalRecordType, txn_id: u64,
        key: &[u8], value: &[u8], now: u64,
    ) -> u64 {
        let lsn = self.next_lsn;
        self.next_lsn += 1;

        let mut record = WalRecord {
            lsn,
            record_type,
            txn_id,
            key: key.to_vec(),
            value: value.to_vec(),
            timestamp: now,
            checksum: 0,
        };
        record.checksum = record.compute_checksum();

        self.stats.bytes_written += record.size() as u64;
        self.stats.records_written += 1;
        self.records.push(record);

        lsn
    }

    /// Truncate records before a given LSN.
    fn truncate_before(&mut self, lsn: u64) {
        self.records.retain(|r| r.lsn >= lsn);
        self.active_txns.retain(|t| !t.committed && !t.aborted);
    }

    /// Pending (uncommitted) transaction count.
    pub fn pending_txns(&self) -> usize {
        self.active_txns.iter().filter(|t| !t.committed && !t.aborted).count()
    }
}

/// CRC32 lookup table (IEEE polynomial).
static CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = 0xEDB88320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};
