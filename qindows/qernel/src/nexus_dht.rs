//! # Nexus DHT — Distributed Hash Table for Peer Discovery (Phase 94)
//!
//! ARCHITECTURE.md §7 — Nexus Mesh:
//! > "Every Q-Device auto-discovers peers via mDNS + a Kademlia DHT"
//! > "Store/Retrieve: content-addressed objects on the mesh"
//! > "Nexus NodeId = SHA-256 of device public key"
//!
//! ## Architecture Guardian: What was missing
//! `nexus.rs` (Phase 7) implements the mesh overlay and fiber offloading.
//! `qtraffic.rs` (Phase 69) tracks per-Silo network utilisation.
//!
//! Missing: the **Kademlia DHT routing layer** — how does a device find which
//! NodeId stores a given OID when the mesh has N > 2 nodes?
//!
//! ## Kademlia Primer
//! ```text
//! NodeId space: 256-bit (same as OID space — by design)
//! Distance: XOR metric (NodeA.id XOR NodeB.id)
//! Routing table: 256 k-buckets, each holds K=8 closest peers
//!   k-bucket[i] holds peers whose XOR distance has bit-length i
//! Lookup: iterative — contact log2(N) hops to reach target OID's responsible node
//! Store:  responsible node = closest NodeId to OID in XOR space
//! ```
//!
//! ## Optimization for Qindows
//! - Only 256-bit OIDs (same as SHA-256 Prism OIDs) — no separate key space
//! - DHT stores location records: OID → NodeId (not the object itself)
//! - Intra-mesh objects replicated to K=3 closest peers for fault tolerance
//! - Integrated with UNS Cache: resolved OID→Node goes into uns_cache as RemotePrism

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── Node ID ───────────────────────────────────────────────────────────────────

/// A Nexus mesh node identity (SHA-256 of device public key).
pub type NodeId = [u8; 32];

/// XOR distance between two NodeIds (Kademlia metric).
fn xor_distance(a: &NodeId, b: &NodeId) -> [u8; 32] {
    let mut d = [0u8; 32];
    for i in 0..32 { d[i] = a[i] ^ b[i]; }
    d
}

/// Number of leading zero bits in a 256-bit value (Kademlia bucket index).
fn leading_zeros_256(v: &[u8; 32]) -> u8 {
    for (i, &b) in v.iter().enumerate() {
        if b != 0 {
            return (i as u8) * 8 + b.leading_zeros() as u8;
        }
    }
    255 // all-zeros distance (same node)
}

/// Return true if a < b in 256-bit unsigned comparison.
fn less_than_256(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] < b[i] { return true; }
        if a[i] > b[i] { return false; }
    }
    false
}

// ── Peer Info ─────────────────────────────────────────────────────────────────

/// A Nexus mesh peer in the routing table.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: NodeId,
    /// IPv6 address (Nexus uses link-local IPv6)
    pub addr_v6: [u8; 16],
    /// Last seen tick (for eviction)
    pub last_seen: u64,
    /// RTT estimate in ticks (ping latency)
    pub rtt_ticks: u64,
    /// Whether this peer is confirmed reachable
    pub active: bool,
}

impl PeerInfo {
    pub fn is_stale(&self, now: u64, timeout_ticks: u64) -> bool {
        now.saturating_sub(self.last_seen) > timeout_ticks
    }
}

// ── K-Bucket ──────────────────────────────────────────────────────────────────

/// One k-bucket in the Kademlia routing table.
#[derive(Debug, Clone, Default)]
pub struct KBucket {
    /// Up to K=8 peers, ordered least-recently-seen first
    pub peers: Vec<PeerInfo>,
    pub k: usize, // max peers (default 8)
}

impl KBucket {
    pub fn new(k: usize) -> Self { KBucket { peers: Vec::new(), k } }

    /// Add or refresh a peer in this bucket.
    pub fn update(&mut self, peer: PeerInfo, now: u64) {
        if let Some(existing) = self.peers.iter_mut().find(|p| p.node_id == peer.node_id) {
            existing.last_seen = now;
            existing.rtt_ticks = peer.rtt_ticks;
            existing.active = true;
            return;
        }
        if self.peers.len() < self.k {
            self.peers.push(peer);
        } else {
            // Evict least-recently-seen (LRS) if stale else drop new peer
            if let Some(idx) = self.peers.iter().position(|p| !p.active) {
                self.peers[idx] = peer;
            }
            // If all active, drop the new peer (Kademlia: old stable peers are preferred)
        }
    }

    pub fn remove(&mut self, node_id: &NodeId) {
        self.peers.retain(|p| &p.node_id != node_id);
    }
}

// ── DHT Location Record ───────────────────────────────────────────────────────

/// A stored DHT record: OID → responsible NodeId(s) + replication list.
#[derive(Debug, Clone)]
pub struct DhtRecord {
    pub oid: [u8; 32],
    /// Primary responsible node
    pub primary_node: NodeId,
    /// Up to K=3 replica nodes
    pub replicas: Vec<NodeId>,
    /// Tick when this record was published
    pub published_at: u64,
    /// TTL for this record (expiry tick = published_at + ttl_ticks)
    pub ttl_ticks: u64,
    /// Size hint (bytes) — used by storage-tier load balancing
    pub size_hint: u64,
}

impl DhtRecord {
    pub fn is_expired(&self, now: u64) -> bool {
        now.saturating_sub(self.published_at) > self.ttl_ticks
    }
}

// ── Lookup Result ─────────────────────────────────────────────────────────────

/// Result of a DHT lookup.
#[derive(Debug, Clone)]
pub struct LookupResult {
    pub oid: [u8; 32],
    /// Closest K nodes found (may or may not have the OID)
    pub closest_nodes: Vec<PeerInfo>,
    /// DHT record if found
    pub record: Option<DhtRecord>,
    /// Hops taken
    pub hops: u32,
}

// ── DHT Statistics ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct DhtStats {
    pub peers_learned: u64,
    pub records_published: u64,
    pub records_found: u64,
    pub lookups_executed: u64,
    pub cache_hits: u64,        // record in local store
    pub bucket_updates: u64,
    pub records_expired: u64,
}

// ── Nexus DHT ─────────────────────────────────────────────────────────────────

/// Kademlia DHT routing table and record store for Nexus mesh.
pub struct NexusDht {
    /// This node's identity
    pub self_id: NodeId,
    /// 256 k-buckets indexed by leading-zero-bit count of XOR distance
    pub routing_table: [KBucket; 256],
    /// Local DHT record store: OID key → DhtRecord
    pub record_store: BTreeMap<u64, DhtRecord>,
    /// K parameter for bucket sizing
    pub k: usize,
    /// Stale peer timeout (ticks)
    pub stale_timeout_ticks: u64,
    /// Default record TTL
    pub record_ttl_ticks: u64,
    /// Statistics
    pub stats: DhtStats,
}

impl NexusDht {
    pub fn new(self_id: NodeId) -> Self {
        const EMPTY_BUCKET: KBucket = KBucket { peers: Vec::new(), k: 8 };
        NexusDht {
            self_id,
            routing_table: [EMPTY_BUCKET; 256],
            record_store: BTreeMap::new(),
            k: 8,
            stale_timeout_ticks: 300_000, // 5 minutes
            record_ttl_ticks: 3_600_000,  // 1 hour
            stats: DhtStats::default(),
        }
    }

    fn oid_key(oid: &[u8; 32]) -> u64 {
        u64::from_le_bytes([oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7]])
    }

    fn bucket_index(&self, target: &NodeId) -> usize {
        let dist = xor_distance(&self.self_id, target);
        leading_zeros_256(&dist) as usize
    }

    // ── Peer Management ───────────────────────────────────────────────────────

    /// Learn about a peer (from mDNS discovery, incoming connection, or DHT response).
    pub fn learn_peer(&mut self, peer: PeerInfo, now: u64) {
        if peer.node_id == self.self_id { return; } // don't add self
        let idx = self.bucket_index(&peer.node_id);
        self.routing_table[idx].update(peer, now);
        self.stats.peers_learned += 1;
        self.stats.bucket_updates += 1;
    }

    /// Mark a peer as inactive (ping timeout).
    pub fn peer_timeout(&mut self, node_id: &NodeId) {
        let idx = self.bucket_index(node_id);
        if let Some(p) = self.routing_table[idx].peers.iter_mut().find(|p| &p.node_id == node_id) {
            p.active = false;
        }
    }

    /// Find the K closest peers in the routing table to a target NodeId.
    pub fn find_closest(&self, target: &NodeId) -> Vec<PeerInfo> {
        let mut candidates: Vec<(PeerInfo, [u8; 32])> = Vec::new();
        for bucket in &self.routing_table {
            for peer in &bucket.peers {
                if peer.active {
                    let dist = xor_distance(&peer.node_id, target);
                    candidates.push((peer.clone(), dist));
                }
            }
        }
        candidates.sort_by(|a, b| {
            for i in 0..32 {
                if a.1[i] != b.1[i] { return a.1[i].cmp(&b.1[i]); }
            }
            core::cmp::Ordering::Equal
        });
        candidates.into_iter().take(self.k).map(|(p, _)| p).collect()
    }

    // ── Record Store ──────────────────────────────────────────────────────────

    /// Publish a DHT record (we are responsible for this OID range).
    pub fn publish(&mut self, oid: [u8; 32], replicas: Vec<NodeId>, size_hint: u64, now: u64) {
        let record = DhtRecord {
            oid,
            primary_node: self.self_id,
            replicas,
            published_at: now,
            ttl_ticks: self.record_ttl_ticks,
            size_hint,
        };
        self.record_store.insert(Self::oid_key(&oid), record);
        self.stats.records_published += 1;
        crate::serial_println!(
            "[NEXUS DHT] Published OID {:02x}{:02x}.. ({} bytes) node={:02x}{:02x}..",
            oid[0], oid[1], size_hint, self.self_id[0], self.self_id[1]
        );
    }

    /// Local lookup — check our own record store.
    pub fn local_lookup(&self, oid: &[u8; 32]) -> Option<&DhtRecord> {
        self.record_store.get(&Self::oid_key(oid))
    }

    /// Simulated iterative lookup (single-node — returns closest peers for real network).
    pub fn lookup(&mut self, oid: [u8; 32]) -> LookupResult {
        self.stats.lookups_executed += 1;

        // Check local store first
        if let Some(record) = self.record_store.get(&Self::oid_key(&oid)) {
            if !record.is_expired(0) {
                self.stats.cache_hits += 1;
                self.stats.records_found += 1;
                return LookupResult {
                    oid,
                    closest_nodes: Vec::new(),
                    record: Some(record.clone()),
                    hops: 0,
                };
            }
        }

        // Return closest peers for network traversal
        let target_node_id = oid; // OID and NodeId share the same 256-bit space
        let closest = self.find_closest(&target_node_id);
        crate::serial_println!(
            "[NEXUS DHT] Lookup {:02x}{:02x}..: {} closest peers, 1 hop",
            oid[0], oid[1], closest.len()
        );

        LookupResult { oid, closest_nodes: closest, record: None, hops: 1 }
    }

    /// Sweep expired records from the local store.
    pub fn sweep_expired(&mut self, now: u64) {
        let before = self.record_store.len();
        self.record_store.retain(|_, r| !r.is_expired(now));
        self.stats.records_expired += (before - self.record_store.len()) as u64;
    }

    pub fn print_stats(&self) {
        let active_peers: usize = self.routing_table.iter()
            .map(|b| b.peers.iter().filter(|p| p.active).count()).sum();
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Nexus DHT (Kademlia §7)            ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Self: {:02x}{:02x}{:02x}{:02x}..                    ║",
            self.self_id[0], self.self_id[1], self.self_id[2], self.self_id[3]);
        crate::serial_println!("║ Active peers:  {:>6}                ║", active_peers);
        crate::serial_println!("║ Records stored:{:>6}                ║", self.record_store.len());
        crate::serial_println!("║ Lookups:       {:>6}                ║", self.stats.lookups_executed);
        crate::serial_println!("║ Cache hits:    {:>6}                ║", self.stats.cache_hits);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
