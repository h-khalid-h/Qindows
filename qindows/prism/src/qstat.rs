//! # Q-Stat — Filesystem Statistics Collector
//!
//! Collects and aggregates filesystem usage statistics
//! per Silo for reporting and quota enforcement (Section 3.28).
//!
//! Features:
//! - Per-Silo file/directory counts
//! - Storage usage tracking
//! - I/O throughput counters
//! - Hot file detection
//! - Historical usage snapshots

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Per-Silo filesystem statistics.
#[derive(Debug, Clone)]
pub struct SiloFsStats {
    pub silo_id: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub total_bytes: u64,
    pub reads: u64,
    pub writes: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub last_updated: u64,
}

/// A hot file entry (frequently accessed).
#[derive(Debug, Clone)]
pub struct HotFile {
    pub path: String,
    pub silo_id: u64,
    pub access_count: u64,
    pub last_access: u64,
}

/// A usage snapshot (point-in-time).
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub timestamp: u64,
    pub silo_id: u64,
    pub total_bytes: u64,
    pub file_count: u64,
}

/// Global statistics.
#[derive(Debug, Clone, Default)]
pub struct GlobalStats {
    pub total_silos_tracked: u64,
    pub snapshots_taken: u64,
}

/// The Q-Stat Manager.
pub struct QStat {
    pub silo_stats: BTreeMap<u64, SiloFsStats>,
    pub hot_files: Vec<HotFile>,
    pub snapshots: Vec<UsageSnapshot>,
    pub max_hot_files: usize,
    pub max_snapshots: usize,
    pub stats: GlobalStats,
}

impl QStat {
    pub fn new() -> Self {
        QStat {
            silo_stats: BTreeMap::new(),
            hot_files: Vec::new(),
            snapshots: Vec::new(),
            max_hot_files: 100,
            max_snapshots: 1000,
            stats: GlobalStats::default(),
        }
    }

    /// Initialize tracking for a Silo.
    pub fn track_silo(&mut self, silo_id: u64) {
        self.silo_stats.entry(silo_id).or_insert(SiloFsStats {
            silo_id, file_count: 0, dir_count: 0, total_bytes: 0,
            reads: 0, writes: 0, bytes_read: 0, bytes_written: 0, last_updated: 0,
        });
        self.stats.total_silos_tracked += 1;
    }

    /// Record a file creation.
    pub fn file_created(&mut self, silo_id: u64, size: u64, now: u64) {
        if let Some(s) = self.silo_stats.get_mut(&silo_id) {
            s.file_count += 1;
            s.total_bytes += size;
            s.last_updated = now;
        }
    }

    /// Record a file deletion.
    pub fn file_deleted(&mut self, silo_id: u64, size: u64, now: u64) {
        if let Some(s) = self.silo_stats.get_mut(&silo_id) {
            s.file_count = s.file_count.saturating_sub(1);
            s.total_bytes = s.total_bytes.saturating_sub(size);
            s.last_updated = now;
        }
    }

    /// Record a read I/O.
    pub fn record_read(&mut self, silo_id: u64, bytes: u64, path: &str, now: u64) {
        if let Some(s) = self.silo_stats.get_mut(&silo_id) {
            s.reads += 1;
            s.bytes_read += bytes;
            s.last_updated = now;
        }
        self.track_hot_file(path, silo_id, now);
    }

    /// Record a write I/O.
    pub fn record_write(&mut self, silo_id: u64, bytes: u64, now: u64) {
        if let Some(s) = self.silo_stats.get_mut(&silo_id) {
            s.writes += 1;
            s.bytes_written += bytes;
            s.last_updated = now;
        }
    }

    /// Take a usage snapshot.
    pub fn snapshot(&mut self, silo_id: u64, now: u64) {
        if let Some(s) = self.silo_stats.get(&silo_id) {
            self.snapshots.push(UsageSnapshot {
                timestamp: now, silo_id,
                total_bytes: s.total_bytes, file_count: s.file_count,
            });
            self.stats.snapshots_taken += 1;
            if self.snapshots.len() > self.max_snapshots {
                self.snapshots.remove(0);
            }
        }
    }

    fn track_hot_file(&mut self, path: &str, silo_id: u64, now: u64) {
        if let Some(hf) = self.hot_files.iter_mut().find(|h| h.path == path && h.silo_id == silo_id) {
            hf.access_count += 1;
            hf.last_access = now;
        } else if self.hot_files.len() < self.max_hot_files {
            self.hot_files.push(HotFile {
                path: String::from(path), silo_id, access_count: 1, last_access: now,
            });
        }
    }
}
