//! # Nexus CRDTs — Conflict-Free Replicated Data Types
//!
//! Enables distributed state synchronization across the Global Mesh
//! without centralized coordination. Used by:
//! - Q-Collab (distributed workspace CRDTs)
//! - Prism sync (device-to-device state merge)
//! - Shadow Sync (mesh-wide configuration)
//!
//! Supported CRDTs:
//! - **G-Counter**: Grow-only counter
//! - **PN-Counter**: Positive-negative counter
//! - **G-Set**: Grow-only set
//! - **OR-Set**: Observed-remove set
//! - **LWW-Register**: Last-writer-wins register

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// A node (replica) identifier.
pub type ReplicaId = u64;

// ─── G-Counter ──────────────────────────────────────────────────────────────

/// A grow-only counter (each replica can only increment).
#[derive(Debug, Clone)]
pub struct GCounter {
    /// Per-replica counts
    pub counts: BTreeMap<ReplicaId, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        GCounter { counts: BTreeMap::new() }
    }

    /// Increment at a replica.
    pub fn increment(&mut self, replica: ReplicaId, amount: u64) {
        let entry = self.counts.entry(replica).or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    /// Get the total value.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Merge with another G-Counter (take max per replica).
    pub fn merge(&mut self, other: &GCounter) {
        for (&replica, &count) in &other.counts {
            let entry = self.counts.entry(replica).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
}

// ─── PN-Counter ─────────────────────────────────────────────────────────────

/// A positive-negative counter (supports increment and decrement).
#[derive(Debug, Clone)]
pub struct PNCounter {
    pub positive: GCounter,
    pub negative: GCounter,
}

impl PNCounter {
    pub fn new() -> Self {
        PNCounter {
            positive: GCounter::new(),
            negative: GCounter::new(),
        }
    }

    pub fn increment(&mut self, replica: ReplicaId, amount: u64) {
        self.positive.increment(replica, amount);
    }

    pub fn decrement(&mut self, replica: ReplicaId, amount: u64) {
        self.negative.increment(replica, amount);
    }

    pub fn value(&self) -> i64 {
        self.positive.value() as i64 - self.negative.value() as i64
    }

    pub fn merge(&mut self, other: &PNCounter) {
        self.positive.merge(&other.positive);
        self.negative.merge(&other.negative);
    }
}

// ─── G-Set ──────────────────────────────────────────────────────────────────

/// A grow-only set (elements can be added but never removed).
#[derive(Debug, Clone)]
pub struct GSet {
    pub elements: BTreeSet<u64>,
}

impl GSet {
    pub fn new() -> Self {
        GSet { elements: BTreeSet::new() }
    }

    pub fn insert(&mut self, element: u64) {
        self.elements.insert(element);
    }

    pub fn contains(&self, element: &u64) -> bool {
        self.elements.contains(element)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn merge(&mut self, other: &GSet) {
        for &e in &other.elements {
            self.elements.insert(e);
        }
    }
}

// ─── OR-Set (Observed-Remove Set) ───────────────────────────────────────────

/// A unique tag for OR-Set elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tag {
    pub replica: ReplicaId,
    pub sequence: u64,
}

/// An observed-remove set (add and remove with causality tracking).
#[derive(Debug, Clone)]
pub struct ORSet {
    /// Element → set of tags (each add creates a unique tag)
    elements: BTreeMap<u64, BTreeSet<Tag>>,
    /// This replica's next sequence number
    replica_id: ReplicaId,
    next_seq: u64,
}

impl ORSet {
    pub fn new(replica_id: ReplicaId) -> Self {
        ORSet {
            elements: BTreeMap::new(),
            replica_id,
            next_seq: 1,
        }
    }

    /// Add an element.
    pub fn insert(&mut self, element: u64) {
        let tag = Tag { replica: self.replica_id, sequence: self.next_seq };
        self.next_seq += 1;
        self.elements.entry(element).or_insert_with(BTreeSet::new).insert(tag);
    }

    /// Remove an element (removes all observed tags).
    pub fn remove(&mut self, element: &u64) {
        self.elements.remove(element);
    }

    /// Check membership.
    pub fn contains(&self, element: &u64) -> bool {
        self.elements.get(element).map_or(false, |tags| !tags.is_empty())
    }

    /// Get all current elements.
    pub fn elements(&self) -> Vec<u64> {
        self.elements.iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(&e, _)| e)
            .collect()
    }

    /// Merge with another OR-Set.
    pub fn merge(&mut self, other: &ORSet) {
        for (&element, other_tags) in &other.elements {
            let our_tags = self.elements.entry(element).or_insert_with(BTreeSet::new);
            for &tag in other_tags {
                our_tags.insert(tag);
            }
        }
    }
}

// ─── LWW-Register (Last-Writer-Wins) ────────────────────────────────────────

/// A last-writer-wins register.
#[derive(Debug, Clone)]
pub struct LWWRegister {
    /// Current value
    pub value: Vec<u8>,
    /// Timestamp of the last write (used for conflict resolution)
    pub timestamp: u64,
    /// Replica that performed the last write
    pub writer: ReplicaId,
}

impl LWWRegister {
    pub fn new() -> Self {
        LWWRegister {
            value: Vec::new(),
            timestamp: 0,
            writer: 0,
        }
    }

    /// Set the value (only wins if timestamp is newer).
    pub fn set(&mut self, value: Vec<u8>, timestamp: u64, writer: ReplicaId) {
        if timestamp > self.timestamp
            || (timestamp == self.timestamp && writer > self.writer)
        {
            self.value = value;
            self.timestamp = timestamp;
            self.writer = writer;
        }
    }

    /// Merge with another register (take the latest write).
    pub fn merge(&mut self, other: &LWWRegister) {
        self.set(other.value.clone(), other.timestamp, other.writer);
    }
}

// ─── CRDT Manager ───────────────────────────────────────────────────────────

/// Statistics for CRDT operations.
#[derive(Debug, Clone, Default)]
pub struct CrdtStats {
    pub merges: u64,
    pub inserts: u64,
    pub removes: u64,
    pub counters_created: u64,
    pub sets_created: u64,
    pub registers_created: u64,
}

/// CRDT instance types.
#[derive(Debug)]
pub enum CrdtInstance {
    Counter(PNCounter),
    Set(ORSet),
    Register(LWWRegister),
}

/// The CRDT Manager — manages named CRDT instances.
pub struct CrdtManager {
    /// Named CRDT instances
    pub instances: BTreeMap<u64, CrdtInstance>,
    /// This node's replica ID
    pub replica_id: ReplicaId,
    /// Statistics
    pub stats: CrdtStats,
}

impl CrdtManager {
    pub fn new(replica_id: ReplicaId) -> Self {
        CrdtManager {
            instances: BTreeMap::new(),
            replica_id,
            stats: CrdtStats::default(),
        }
    }

    /// Create a new PN-Counter.
    pub fn create_counter(&mut self, id: u64) {
        self.instances.insert(id, CrdtInstance::Counter(PNCounter::new()));
        self.stats.counters_created += 1;
    }

    /// Create a new OR-Set.
    pub fn create_set(&mut self, id: u64) {
        self.instances.insert(id, CrdtInstance::Set(ORSet::new(self.replica_id)));
        self.stats.sets_created += 1;
    }

    /// Create a new LWW-Register.
    pub fn create_register(&mut self, id: u64) {
        self.instances.insert(id, CrdtInstance::Register(LWWRegister::new()));
        self.stats.registers_created += 1;
    }

    /// Increment a counter.
    pub fn counter_increment(&mut self, id: u64, amount: u64) {
        if let Some(CrdtInstance::Counter(counter)) = self.instances.get_mut(&id) {
            counter.increment(self.replica_id, amount);
            self.stats.inserts += 1;
        }
    }

    /// Get a counter value.
    pub fn counter_value(&self, id: u64) -> Option<i64> {
        match self.instances.get(&id)? {
            CrdtInstance::Counter(c) => Some(c.value()),
            _ => None,
        }
    }
}
