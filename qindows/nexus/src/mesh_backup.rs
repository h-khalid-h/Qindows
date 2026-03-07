//! # Mesh Backup — Distributed Incremental Backup
//!
//! Backs up Q-Objects across mesh nodes with incremental
//! snapshots and deduplication (Section 11.13).
//!
//! Features:
//! - Content-addressed backup blocks
//! - Incremental change tracking
//! - Multi-node redundancy
//! - Backup scheduling
//! - Restore from any snapshot

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Backup state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupState {
    Idle,
    Running,
    Completed,
    Failed,
}

/// A backup snapshot.
#[derive(Debug, Clone)]
pub struct BackupSnapshot {
    pub id: u64,
    pub silo_id: u64,
    pub timestamp: u64,
    pub blocks: Vec<u64>,
    pub total_bytes: u64,
    pub incremental: bool,
    pub parent_snapshot: Option<u64>,
    pub state: BackupState,
}

/// A backup block (content-addressed).
#[derive(Debug, Clone)]
pub struct BackupBlock {
    pub hash: u64,
    pub size: u32,
    pub ref_count: u32,
    pub nodes: Vec<[u8; 32]>,
}

/// Backup statistics.
#[derive(Debug, Clone, Default)]
pub struct BackupStats {
    pub snapshots_created: u64,
    pub blocks_stored: u64,
    pub blocks_deduped: u64,
    pub bytes_backed_up: u64,
    pub restores: u64,
}

/// The Mesh Backup Manager.
pub struct MeshBackup {
    pub snapshots: BTreeMap<u64, BackupSnapshot>,
    pub blocks: BTreeMap<u64, BackupBlock>,
    next_snapshot_id: u64,
    pub max_snapshots_per_silo: usize,
    pub stats: BackupStats,
}

impl MeshBackup {
    pub fn new() -> Self {
        MeshBackup {
            snapshots: BTreeMap::new(),
            blocks: BTreeMap::new(),
            next_snapshot_id: 1,
            max_snapshots_per_silo: 100,
            stats: BackupStats::default(),
        }
    }

    /// Start a new backup snapshot.
    pub fn start_snapshot(&mut self, silo_id: u64, parent: Option<u64>, now: u64) -> u64 {
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;

        self.snapshots.insert(id, BackupSnapshot {
            id, silo_id, timestamp: now, blocks: Vec::new(),
            total_bytes: 0, incremental: parent.is_some(),
            parent_snapshot: parent, state: BackupState::Running,
        });

        self.stats.snapshots_created += 1;
        id
    }

    /// Add a block to a running snapshot.
    pub fn add_block(&mut self, snapshot_id: u64, hash: u64, size: u32, node: [u8; 32]) {
        // Dedup: check if block already exists
        if let Some(block) = self.blocks.get_mut(&hash) {
            block.ref_count += 1;
            if !block.nodes.contains(&node) {
                block.nodes.push(node);
            }
            self.stats.blocks_deduped += 1;
        } else {
            self.blocks.insert(hash, BackupBlock {
                hash, size, ref_count: 1, nodes: alloc::vec![node],
            });
            self.stats.blocks_stored += 1;
        }

        if let Some(snap) = self.snapshots.get_mut(&snapshot_id) {
            if !snap.blocks.contains(&hash) {
                snap.blocks.push(hash);
            }
            snap.total_bytes += size as u64;
            self.stats.bytes_backed_up += size as u64;
        }
    }

    /// Complete a snapshot.
    pub fn complete(&mut self, snapshot_id: u64) {
        if let Some(snap) = self.snapshots.get_mut(&snapshot_id) {
            snap.state = BackupState::Completed;
        }

        // Enforce per-Silo limit
        if let Some(snap) = self.snapshots.get(&snapshot_id) {
            let silo = snap.silo_id;
            let silo_snaps: Vec<u64> = self.snapshots.values()
                .filter(|s| s.silo_id == silo && s.state == BackupState::Completed)
                .map(|s| s.id)
                .collect();
            if silo_snaps.len() > self.max_snapshots_per_silo {
                // Remove oldest
                if let Some(&oldest) = silo_snaps.first() {
                    self.remove_snapshot(oldest);
                }
            }
        }
    }

    /// Remove a snapshot and decrement block refs.
    fn remove_snapshot(&mut self, snapshot_id: u64) {
        if let Some(snap) = self.snapshots.remove(&snapshot_id) {
            for hash in &snap.blocks {
                if let Some(block) = self.blocks.get_mut(hash) {
                    block.ref_count = block.ref_count.saturating_sub(1);
                    if block.ref_count == 0 {
                        self.blocks.remove(hash);
                    }
                }
            }
        }
    }

    /// Get all snapshots for a Silo.
    pub fn silo_snapshots(&self, silo_id: u64) -> Vec<&BackupSnapshot> {
        self.snapshots.values()
            .filter(|s| s.silo_id == silo_id)
            .collect()
    }
}
