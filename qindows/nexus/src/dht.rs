//! # Distributed Hash Table (DHT)
//!
//! Kademlia-based DHT for the Global Mesh.
//! Provides O(log n) lookup for any Prism object across the
//! planetary network — no central server needed.

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// Node ID in the DHT — 256-bit identifier.
pub type NodeId = [u8; 32];

/// Key-value entry in the DHT.
#[derive(Debug, Clone)]
pub struct DhtEntry {
    /// Object key (typically a Prism OID)
    pub key: [u8; 32],
    /// Node ID that stores this value
    pub owner: NodeId,
    /// Timestamp of last update
    pub timestamp: u64,
    /// Entry size in bytes (metadata only — actual data via Q-Fabric)
    pub size: u64,
    /// Replication factor (how many nodes hold a copy)
    pub replicas: u8,
}

/// A bucket in the Kademlia routing table.
/// Each bucket holds up to K contacts at a specific XOR distance.
const K_BUCKET_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct KBucket {
    /// Contacts in this bucket (sorted by last-seen time)
    pub contacts: Vec<Contact>,
    /// Maximum size
    pub max_size: usize,
}

/// A contact in the routing table.
#[derive(Debug, Clone)]
pub struct Contact {
    pub node_id: NodeId,
    /// Network address (simplified as a u64 for now)
    pub address: u64,
    /// Latency in microseconds
    pub latency_us: u64,
    /// Last time we heard from this node
    pub last_seen: u64,
}

impl KBucket {
    pub fn new() -> Self {
        KBucket {
            contacts: Vec::with_capacity(K_BUCKET_SIZE),
            max_size: K_BUCKET_SIZE,
        }
    }

    /// Add or update a contact.
    pub fn update(&mut self, contact: Contact) {
        // Check if contact already exists
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.node_id == contact.node_id) {
            existing.last_seen = contact.last_seen;
            existing.latency_us = contact.latency_us;
            return;
        }

        // Add new contact if bucket isn't full
        if self.contacts.len() < self.max_size {
            self.contacts.push(contact);
        }
        // If full: could ping least-recently-seen and replace if dead
    }

    /// Get the N closest contacts to a target ID.
    pub fn closest(&self, target: &NodeId, n: usize) -> Vec<&Contact> {
        let mut sorted: Vec<&Contact> = self.contacts.iter().collect();
        sorted.sort_by_key(|c| xor_distance(&c.node_id, target));
        sorted.truncate(n);
        sorted
    }
}

/// XOR distance metric (core of Kademlia).
///
/// Two IDs that share more prefix bits are "closer."
/// This creates a binary tree structure over the ID space.
pub fn xor_distance(a: &NodeId, b: &NodeId) -> [u8; 32] {
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = a[i] ^ b[i];
    }
    result
}

/// Count the number of leading zero bits in a distance.
/// This determines which k-bucket a node belongs to.
pub fn leading_zeros(distance: &[u8; 32]) -> u32 {
    let mut zeros = 0u32;
    for &byte in distance {
        if byte == 0 {
            zeros += 8;
        } else {
            zeros += byte.leading_zeros();
            break;
        }
    }
    zeros
}

/// The Kademlia DHT routing table.
pub struct RoutingTable {
    /// Our node ID
    pub local_id: NodeId,
    /// 256 k-buckets (one for each bit of the ID space)
    pub buckets: Vec<KBucket>,
}

impl RoutingTable {
    pub fn new(local_id: NodeId) -> Self {
        let mut buckets = Vec::with_capacity(256);
        for _ in 0..256 {
            buckets.push(KBucket::new());
        }

        RoutingTable {
            local_id,
            buckets,
        }
    }

    /// Add a contact to the appropriate bucket.
    pub fn add_contact(&mut self, contact: Contact) {
        let distance = xor_distance(&self.local_id, &contact.node_id);
        let bucket_idx = leading_zeros(&distance) as usize;
        if bucket_idx < 256 {
            self.buckets[bucket_idx].update(contact);
        }
    }

    /// Find the K closest contacts to a target key.
    ///
    /// This is the FIND_NODE RPC in Kademlia.
    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<&Contact> {
        let mut all_contacts: Vec<&Contact> = Vec::new();

        for bucket in &self.buckets {
            for contact in &bucket.contacts {
                all_contacts.push(contact);
            }
        }

        all_contacts.sort_by_key(|c| xor_distance(&c.node_id, target));
        all_contacts.truncate(count);
        all_contacts
    }

    /// Count total contacts across all buckets.
    pub fn total_contacts(&self) -> usize {
        self.buckets.iter().map(|b| b.contacts.len()).sum()
    }
}

/// DHT Operations
pub struct Dht {
    /// Routing table
    pub routing: RoutingTable,
    /// Locally stored entries
    pub local_store: Vec<DhtEntry>,
}

impl Dht {
    pub fn new(local_id: NodeId) -> Self {
        Dht {
            routing: RoutingTable::new(local_id),
            local_store: Vec::new(),
        }
    }

    /// Store a value in the DHT.
    ///
    /// In production: this would also replicate to the K closest nodes.
    pub fn store(&mut self, key: [u8; 32], size: u64) {
        self.local_store.push(DhtEntry {
            key,
            owner: self.routing.local_id,
            timestamp: 0,
            size,
            replicas: 1,
        });
    }

    /// Lookup a value by key.
    ///
    /// Checks local store first, then would iteratively query
    /// closer and closer nodes (FIND_VALUE RPC).
    pub fn lookup(&self, key: &[u8; 32]) -> Option<&DhtEntry> {
        self.local_store.iter().find(|e| &e.key == key)
    }

    /// Get the number of locally stored entries.
    pub fn local_count(&self) -> usize {
        self.local_store.len()
    }
}
