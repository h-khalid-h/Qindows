//! # Silo Snapshot — Checkpoint / Restore
//!
//! Saves and restores the full state of a Q-Silo (Section 2.2).
//! Used for instant app resume, live migration between devices,
//! and rollback after failed updates.
//!
//! A snapshot captures:
//! - Memory pages (copy-on-write delta)
//! - Open file handles (Prism OID references)
//! - Thread state (register context)
//! - Network connections (Q-Fabric session tokens)
//! - Capability tokens (active grants)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Snapshot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapState {
    Creating,
    Ready,
    Restoring,
    Failed,
    Expired,
}

/// A captured thread context.
#[derive(Debug, Clone)]
pub struct ThreadContext {
    pub thread_id: u64,
    pub rip: u64,
    pub rsp: u64,
    pub registers: [u64; 16],
    pub flags: u64,
}

/// A memory page delta.
#[derive(Debug, Clone)]
pub struct PageDelta {
    pub vaddr: u64,
    pub data_hash: [u8; 32],
    pub size: u64,
    pub dirty: bool,
}

/// A file handle reference.
#[derive(Debug, Clone)]
pub struct FileRef {
    pub fd: u64,
    pub oid: u64,
    pub offset: u64,
    pub mode: u8, // 1=read, 2=write, 3=rw
}

/// A Silo snapshot.
#[derive(Debug, Clone)]
pub struct SiloSnapshot {
    pub id: u64,
    pub silo_id: u64,
    pub name: String,
    pub state: SnapState,
    pub created_at: u64,
    pub threads: Vec<ThreadContext>,
    pub pages: Vec<PageDelta>,
    pub files: Vec<FileRef>,
    pub capability_tokens: Vec<u64>,
    /// Total snapshot size (bytes)
    pub total_size: u64,
    /// Parent snapshot (for incremental)
    pub parent_id: Option<u64>,
}

/// Snapshot statistics.
#[derive(Debug, Clone, Default)]
pub struct SnapStats {
    pub snapshots_created: u64,
    pub snapshots_restored: u64,
    pub snapshots_deleted: u64,
    pub bytes_saved: u64,
    pub bytes_restored: u64,
    pub incremental_saves: u64,
}

/// The Silo Snapshot Manager.
pub struct SnapshotManager {
    pub snapshots: BTreeMap<u64, SiloSnapshot>,
    /// Silo → latest snapshot ID
    pub latest: BTreeMap<u64, u64>,
    next_id: u64,
    /// Max snapshots per Silo
    pub max_per_silo: usize,
    pub stats: SnapStats,
}

impl SnapshotManager {
    pub fn new() -> Self {
        SnapshotManager {
            snapshots: BTreeMap::new(),
            latest: BTreeMap::new(),
            next_id: 1,
            max_per_silo: 10,
            stats: SnapStats::default(),
        }
    }

    /// Create a snapshot of a Silo.
    pub fn create(
        &mut self,
        silo_id: u64,
        name: &str,
        threads: Vec<ThreadContext>,
        pages: Vec<PageDelta>,
        files: Vec<FileRef>,
        caps: Vec<u64>,
        now: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let parent_id = self.latest.get(&silo_id).copied();
        let total_size: u64 = pages.iter().map(|p| p.size).sum();

        self.snapshots.insert(id, SiloSnapshot {
            id, silo_id,
            name: String::from(name),
            state: SnapState::Ready,
            created_at: now,
            threads, pages, files,
            capability_tokens: caps,
            total_size, parent_id,
        });

        self.latest.insert(silo_id, id);
        self.stats.snapshots_created += 1;
        self.stats.bytes_saved += total_size;

        if parent_id.is_some() {
            self.stats.incremental_saves += 1;
        }

        // Trim old snapshots for this Silo
        self.trim_silo(silo_id);

        id
    }

    /// Restore a Silo from a snapshot.
    pub fn restore(&mut self, snap_id: u64) -> Result<&SiloSnapshot, &'static str> {
        let snap = self.snapshots.get_mut(&snap_id)
            .ok_or("Snapshot not found")?;
        if snap.state != SnapState::Ready {
            return Err("Snapshot not in ready state");
        }
        snap.state = SnapState::Restoring;
        self.stats.snapshots_restored += 1;
        self.stats.bytes_restored += snap.total_size;
        snap.state = SnapState::Ready; // Mark ready again after restore
        Ok(snap)
    }

    /// Delete a snapshot.
    pub fn delete(&mut self, snap_id: u64) {
        self.snapshots.remove(&snap_id);
        self.stats.snapshots_deleted += 1;
    }

    /// Trim old snapshots for a Silo.
    fn trim_silo(&mut self, silo_id: u64) {
        let silo_snaps: Vec<u64> = self.snapshots.values()
            .filter(|s| s.silo_id == silo_id)
            .map(|s| s.id)
            .collect();

        if silo_snaps.len() > self.max_per_silo {
            let to_remove = silo_snaps.len() - self.max_per_silo;
            for &id in silo_snaps.iter().take(to_remove) {
                self.snapshots.remove(&id);
                self.stats.snapshots_deleted += 1;
            }
        }
    }

    /// List snapshots for a Silo.
    pub fn list(&self, silo_id: u64) -> Vec<&SiloSnapshot> {
        self.snapshots.values()
            .filter(|s| s.silo_id == silo_id)
            .collect()
    }
}
