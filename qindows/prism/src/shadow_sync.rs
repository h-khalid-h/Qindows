//! # Prism Shadow Sync — Cross-Device Object Synchronization
//!
//! Keeps Prism Objects in sync across all of a user's Qindows devices.
//! When you edit a file on your desktop, the change propagates to your
//! laptop, phone, and cloud backup automatically.
//!
//! Architecture:
//! - Each device has a **Sync Replica** with a unique ID
//! - Changes create **Sync Entries** (object ID + version + delta)
//! - Entries propagate via Q-Fabric to all replicas
//! - Conflicts resolved via LWW (Last-Writer-Wins) or CRDT merge
//! - Bandwidth-efficient: only deltas are transmitted

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Sync direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// Push local changes to remote
    Push,
    /// Pull remote changes to local
    Pull,
    /// Bidirectional
    BiDir,
}

/// Sync entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Pending sync
    Pending,
    /// Being transferred
    InFlight,
    /// Synced successfully
    Synced,
    /// Conflict detected (needs resolution)
    Conflict,
    /// Failed (will retry)
    Failed,
}

/// A sync entry — one object change to propagate.
#[derive(Debug, Clone)]
pub struct SyncEntry {
    /// Object ID
    pub oid: u64,
    /// Local version
    pub local_version: u64,
    /// Remote version (if known)
    pub remote_version: u64,
    /// Delta size (bytes)
    pub delta_size: u64,
    /// State
    pub state: SyncState,
    /// Timestamp of the change
    pub changed_at: u64,
    /// Which replica made the change
    pub origin_replica: u64,
}

/// A sync replica (one device).
#[derive(Debug, Clone)]
pub struct SyncReplica {
    /// Replica ID (device identity)
    pub id: u64,
    /// Device name
    pub name: String,
    /// Last sync timestamp
    pub last_sync: u64,
    /// Is this replica online?
    pub online: bool,
    /// Latency to this replica (ms)
    pub latency_ms: u32,
    /// Objects synced
    pub objects_synced: u64,
    /// Bytes transferred
    pub bytes_transferred: u64,
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Last writer wins (by timestamp)
    LastWriterWins,
    /// Keep both versions (create fork)
    KeepBoth,
    /// Merge via CRDT (if supported)
    CrdtMerge,
    /// Prompt user
    AskUser,
}

/// Sync statistics.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub entries_pushed: u64,
    pub entries_pulled: u64,
    pub bytes_pushed: u64,
    pub bytes_pulled: u64,
    pub conflicts_detected: u64,
    pub conflicts_resolved: u64,
    pub sync_cycles: u64,
}

/// The Shadow Sync Engine.
pub struct ShadowSync {
    /// This device's replica ID
    pub local_replica: u64,
    /// Known replicas (other devices)
    pub replicas: BTreeMap<u64, SyncReplica>,
    /// Pending sync entries (outgoing)
    pub outbox: Vec<SyncEntry>,
    /// Incoming sync entries
    pub inbox: Vec<SyncEntry>,
    /// Object version map (OID → local version)
    pub versions: BTreeMap<u64, u64>,
    /// Conflict resolution strategy
    pub conflict_strategy: ConflictStrategy,
    /// Sync direction
    pub direction: SyncDirection,
    /// Statistics
    pub stats: SyncStats,
}

impl ShadowSync {
    pub fn new(local_replica: u64) -> Self {
        ShadowSync {
            local_replica,
            replicas: BTreeMap::new(),
            outbox: Vec::new(),
            inbox: Vec::new(),
            versions: BTreeMap::new(),
            conflict_strategy: ConflictStrategy::LastWriterWins,
            direction: SyncDirection::BiDir,
            stats: SyncStats::default(),
        }
    }

    /// Register a remote replica (another device).
    pub fn add_replica(&mut self, replica: SyncReplica) {
        self.replicas.insert(replica.id, replica);
    }

    /// Record a local object change (adds to outbox).
    pub fn on_local_change(&mut self, oid: u64, delta_size: u64, now: u64) {
        let version = self.versions.entry(oid).or_insert(0);
        *version += 1;
        let v = *version;

        self.outbox.push(SyncEntry {
            oid,
            local_version: v,
            remote_version: 0,
            delta_size,
            state: SyncState::Pending,
            changed_at: now,
            origin_replica: self.local_replica,
        });
    }

    /// Receive a remote change (adds to inbox).
    pub fn on_remote_change(&mut self, entry: SyncEntry) {
        self.inbox.push(entry);
    }

    /// Process outbox — push local changes to replicas.
    pub fn push(&mut self) -> usize {
        if self.direction == SyncDirection::Pull {
            return 0;
        }

        let mut pushed = 0;
        for entry in &mut self.outbox {
            if entry.state == SyncState::Pending {
                entry.state = SyncState::InFlight;
                // In production: send delta via Q-Fabric to all replicas
                entry.state = SyncState::Synced;
                self.stats.entries_pushed += 1;
                self.stats.bytes_pushed = self.stats.bytes_pushed
                    .saturating_add(entry.delta_size);
                pushed += 1;
            }
        }
        // Remove synced entries
        self.outbox.retain(|e| e.state != SyncState::Synced);
        pushed
    }

    /// Process inbox — apply remote changes locally.
    pub fn pull(&mut self) -> usize {
        if self.direction == SyncDirection::Push {
            return 0;
        }

        let mut pulled = 0;
        let mut conflicts = Vec::new();

        for entry in &mut self.inbox {
            let local_version = self.versions.get(&entry.oid).copied().unwrap_or(0);

            if entry.remote_version > 0 && local_version > entry.remote_version {
                // Conflict — local was modified since remote's base
                entry.state = SyncState::Conflict;
                conflicts.push(entry.oid);
                self.stats.conflicts_detected += 1;
            } else {
                // No conflict — apply the remote change
                self.versions.insert(entry.oid, entry.local_version);
                entry.state = SyncState::Synced;
                self.stats.entries_pulled += 1;
                self.stats.bytes_pulled = self.stats.bytes_pulled
                    .saturating_add(entry.delta_size);
                pulled += 1;
            }
        }

        // Resolve conflicts
        for oid in &conflicts {
            self.resolve_conflict(*oid);
        }

        // Remove synced entries
        self.inbox.retain(|e| e.state != SyncState::Synced);
        pulled
    }

    /// Resolve a conflict using the configured strategy.
    fn resolve_conflict(&mut self, oid: u64) {
        match self.conflict_strategy {
            ConflictStrategy::LastWriterWins => {
                // Find the newest version in inbox
                if let Some(entry) = self.inbox.iter_mut()
                    .filter(|e| e.oid == oid && e.state == SyncState::Conflict)
                    .max_by_key(|e| e.changed_at)
                {
                    self.versions.insert(oid, entry.local_version);
                    entry.state = SyncState::Synced;
                    self.stats.conflicts_resolved += 1;
                }
            }
            ConflictStrategy::KeepBoth => {
                // Create a fork — both versions coexist
                // In production: create a new OID for the local version
                for entry in self.inbox.iter_mut()
                    .filter(|e| e.oid == oid && e.state == SyncState::Conflict)
                {
                    entry.state = SyncState::Synced;
                    self.stats.conflicts_resolved += 1;
                }
            }
            ConflictStrategy::CrdtMerge => {
                // Delegate to nexus::crdt for automatic merge
                for entry in self.inbox.iter_mut()
                    .filter(|e| e.oid == oid && e.state == SyncState::Conflict)
                {
                    entry.state = SyncState::Synced;
                    self.stats.conflicts_resolved += 1;
                }
            }
            ConflictStrategy::AskUser => {
                // Leave as Conflict — UI will prompt user
            }
        }
    }

    /// Run a full sync cycle (push + pull).
    pub fn sync_cycle(&mut self) -> (usize, usize) {
        let pushed = self.push();
        let pulled = self.pull();
        self.stats.sync_cycles += 1;
        (pushed, pulled)
    }

    /// Get sync status summary.
    pub fn status(&self) -> (usize, usize, usize) {
        let pending = self.outbox.len();
        let incoming = self.inbox.len();
        let conflicts = self.inbox.iter()
            .filter(|e| e.state == SyncState::Conflict)
            .count();
        (pending, incoming, conflicts)
    }
}
