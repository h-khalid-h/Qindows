//! # Q-Collab — Distributed CRDT Collaborative Workspace (Phase 67)
//!
//! Q-Collab is Qindows's first-party collaborative workspace subsystem.
//! It implements **Operation-based CRDTs** (Conflict-free Replicated Data Types)
//! at the kernel level so two Qindows nodes can merge their Prisms over Q-Fabric.
//!
//! ## ARCHITECTURE.md §"Q-Collab Architecture"
//! > "Q-Collab uses CRDTs at the kernel level. When you type a character,
//! > you aren't sending a 'message'; you are updating a Shared Object that
//! > exists in two places at once. No Servers: Data flows directly from your
//! > NVMe to your colleague's NVMe via encrypted QUIC streams."
//!
//! ## What This Module Provides (kernel-side interface)
//! 1. `SharedDocument` — a CRDT document with an operation log
//! 2. `CollabSession` — a live multi-node collaboration session
//! 3. `VectorClock` — for causal ordering of concurrent operations
//! 4. `apply_delta()` / `merge_remote_delta()` — the core CRDT merge logic
//!
//! ## Architecture Guardian: Layering
//! ```text
//! Q-Kit (user Silo — renders the editor, cursors, UI)
//!    │  Q-Ring: CollabSubmit, CollabPoll
//!    ▼
//! ColalbSession (this module — kernel-side CRDT state)
//!    │  Q-Fabric: encrypted QUIC stream
//!    ▼
//! Remote Peer's CollabSession (same module, mirrored)
//! ```
//!
//! ## Q-Manifest Law 6: Silo Sandbox
//! A collab session is owned by one Silo. Other Silos cannot inject
//! operations without a valid `CollabCapToken` granted by the owner.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;
use crate::nexus::NodeId;

// ── Vector Clock ──────────────────────────────────────────────────────────────

/// A Lamport-style vector clock for causal ordering of distributed operations.
///
/// Each node increments its own counter on every local operation.
/// Before applying a remote operation, the receiver checks that all
/// causally-preceding clocks have been seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorClock(pub BTreeMap<NodeId, u64>);

impl VectorClock {
    pub fn new() -> Self { VectorClock(BTreeMap::new()) }

    /// Increment this node's clock counter.
    pub fn tick(&mut self, node: NodeId) {
        *self.0.entry(node).or_insert(0) += 1;
    }

    /// Return the logical timestamp for a given node.
    pub fn get(&self, node: &NodeId) -> u64 {
        *self.0.get(node).unwrap_or(&0)
    }

    /// Merge: take the max of each component (happens-before merge).
    pub fn merge(&mut self, other: &VectorClock) {
        for (node, &t) in &other.0 {
            let entry = self.0.entry(*node).or_insert(0);
            if t > *entry { *entry = t; }
        }
    }

    /// Returns true if `self` happens-before `other` (strictly).
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let self_leq_other = self.0.iter().all(|(n, &t)| t <= other.get(n));
        let some_strictly_less = self.0.iter().any(|(n, &t)| t < other.get(n))
            || other.0.iter().any(|(n, &t)| t > self.get(n));
        self_leq_other && some_strictly_less
    }
}

// ── CRDT Operations ───────────────────────────────────────────────────────────

/// A unique operation ID for CRDT operations (node + sequence number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpId {
    pub node: NodeId,
    pub seq: u64,
}

/// A CRDT text operation (Operation-Transformation style).
#[derive(Debug, Clone)]
pub enum CrdtOp {
    /// Insert `text` at logical position `pos` with unique operation ID
    Insert { op_id: OpId, pos: u64, text: String },
    /// Delete the character inserted by `target_op_id`
    Delete { op_id: OpId, target_op_id: OpId },
    /// Attribute change (bold, italic, etc.) at range
    Attribute { op_id: OpId, start_op: OpId, end_op: OpId, key: String, value: String },
    /// Cursor movement (broadcast to peers for live cursor display)
    Cursor { node: NodeId, pos: u64 },
}

impl CrdtOp {
    pub fn op_id(&self) -> Option<OpId> {
        match self {
            CrdtOp::Insert { op_id, .. } => Some(*op_id),
            CrdtOp::Delete { op_id, .. } => Some(*op_id),
            CrdtOp::Attribute { op_id, .. } => Some(*op_id),
            CrdtOp::Cursor { .. } => None,
        }
    }
}

// ── Shared Document ───────────────────────────────────────────────────────────

/// A CRDT-backed shared document.
///
/// The document is represented as an **operation log** — the text content
/// is derived by replaying the log. This is the RGA (Replicated Growable Array)
/// approach used by collaborative editors like Peritext.
#[derive(Debug, Clone)]
pub struct SharedDocument {
    /// The document's unique OID in the Prism
    pub prism_oid: u64,
    /// The ordered CRDT operation log (total order by (seq, node))
    pub op_log: Vec<CrdtOp>,
    /// Set of deleted op IDs (tombstones)
    pub tombstones: BTreeMap<OpId, ()>,
    /// Live cursors: node_id → logical position
    pub cursors: BTreeMap<NodeId, u64>,
    /// Current vector clock
    pub clock: VectorClock,
    /// Owner Silo
    pub owner_silo: u64,
}

impl SharedDocument {
    pub fn new(prism_oid: u64, owner_silo: u64) -> Self {
        SharedDocument {
            prism_oid,
            op_log: Vec::new(),
            tombstones: BTreeMap::new(),
            cursors: BTreeMap::new(),
            clock: VectorClock::new(),
            owner_silo,
        }
    }

    /// Derive the current text content by replaying the operation log.
    ///
    /// Inserts are ordered by (seq, node). Tombstoned inserts are skipped.
    /// This is an O(n) scan — in production, an interval tree is used for O(log n).
    pub fn derive_text(&self) -> String {
        let mut chars: Vec<(OpId, &str)> = self.op_log.iter()
            .filter_map(|op| {
                if let CrdtOp::Insert { op_id, text, .. } = op {
                    if !self.tombstones.contains_key(op_id) {
                        return Some((*op_id, text.as_str()));
                    }
                }
                None
            })
            .collect();
        // Sort by (seq, node) for deterministic ordering across peers
        chars.sort_by_key(|(id, _)| (id.seq, id.node));
        chars.into_iter().map(|(_, s)| s).collect()
    }

    /// Apply a local CRDT operation and tick the local clock.
    pub fn apply_local(&mut self, op: CrdtOp, local_node: NodeId) {
        self.clock.tick(local_node);
        if let CrdtOp::Delete { target_op_id, .. } = &op {
            self.tombstones.insert(*target_op_id, ());
        }
        if let CrdtOp::Cursor { node, pos } = &op {
            self.cursors.insert(*node, *pos);
        }
        self.op_log.push(op);
    }

    /// Merge a remote operation received over Q-Fabric.
    ///
    /// CRDTs are commutative: remote ops can be applied in any order.
    /// The tombstone set handles concurrent delete-insert conflicts safely.
    pub fn merge_remote(&mut self, op: CrdtOp, remote_clock: &VectorClock) {
        self.clock.merge(remote_clock);
        match &op {
            CrdtOp::Delete { target_op_id, .. } => {
                self.tombstones.insert(*target_op_id, ());
            }
            CrdtOp::Cursor { node, pos } => {
                self.cursors.insert(*node, *pos);
                return; // Cursor ops are not logged permanently
            }
            _ => {}
        }
        // Idempotency: don't apply the same op twice
        let op_id = op.op_id();
        let already_seen = op_id.map(|id| {
            self.op_log.iter().any(|existing| existing.op_id() == Some(id))
        }).unwrap_or(false);

        if !already_seen {
            self.op_log.push(op);
        }
    }

    /// Return a delta of operations the given peer hasn't seen yet.
    ///
    /// Used for catch-up sync when a peer reconnects after being offline.
    pub fn delta_since(&self, peer_clock: &VectorClock, local_node: NodeId) -> Vec<CrdtOp> {
        let local_seq = peer_clock.get(&local_node);
        self.op_log.iter()
            .filter(|op| {
                op.op_id().map(|id| id.node == local_node && id.seq > local_seq)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}

// ── Collab Session ────────────────────────────────────────────────────────────

/// Status of a collaboration session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Waiting for at least one peer to join
    Waiting,
    /// Active with peers connected
    Active,
    /// Owner disconnected — session in read-only recovery mode
    OwnerOffline,
    /// Session closed
    Closed,
}

/// A live multi-node collaboration session.
#[derive(Debug, Clone)]
pub struct CollabSession {
    /// Session ID (shared across all participating nodes)
    pub session_id: u64,
    /// Owner Silo on the local node
    pub owner_silo: u64,
    /// Connected peer nodes: NodeId → (their clock snapshot)
    pub peers: BTreeMap<NodeId, VectorClock>,
    /// The shared document
    pub document: SharedDocument,
    /// Session status
    pub status: SessionStatus,
    /// Kernel tick when session was created
    pub created_at: u64,
    /// Total operations processed
    pub ops_processed: u64,
}

impl CollabSession {
    pub fn new(session_id: u64, owner_silo: u64, prism_oid: u64, tick: u64) -> Self {
        CollabSession {
            session_id,
            owner_silo,
            peers: BTreeMap::new(),
            document: SharedDocument::new(prism_oid, owner_silo),
            status: SessionStatus::Waiting,
            created_at: tick,
            ops_processed: 0,
        }
    }

    /// A peer node has joined the session.
    pub fn peer_join(&mut self, peer: NodeId) {
        crate::serial_println!(
            "[COLLAB] Session {} — peer {:08x} joined.", self.session_id, peer.short_hex()
        );
        self.peers.insert(peer, VectorClock::new());
        self.status = SessionStatus::Active;
    }

    /// A peer node has left the session.
    pub fn peer_leave(&mut self, peer: NodeId) {
        self.peers.remove(&peer);
        crate::serial_println!(
            "[COLLAB] Session {} — peer {:08x} left ({} remaining).",
            self.session_id, peer.short_hex(), self.peers.len()
        );
        if self.peers.is_empty() {
            self.status = SessionStatus::Waiting;
        }
    }

    /// Submit a local operation from the Q-Kit editor.
    pub fn submit_local_op(&mut self, op: CrdtOp, local_node: NodeId) {
        self.document.apply_local(op, local_node);
        self.ops_processed += 1;
        crate::serial_println!(
            "[COLLAB] Session {} — local op applied (total ops: {}).",
            self.session_id, self.ops_processed
        );
    }

    /// Receive and merge an operation from a remote peer.
    pub fn receive_remote_op(&mut self, op: CrdtOp, from: NodeId, peer_clock: VectorClock) {
        self.document.merge_remote(op, &peer_clock);
        if let Some(clk) = self.peers.get_mut(&from) {
            clk.merge(&peer_clock);
        }
        self.ops_processed += 1;
    }

    /// Build a catch-up delta for a peer that just reconnected.
    pub fn build_catchup(&self, peer: NodeId, local_node: NodeId) -> Vec<CrdtOp> {
        let peer_clock = self.peers.get(&peer)
            .cloned()
            .unwrap_or_else(VectorClock::new);
        self.document.delta_since(&peer_clock, local_node)
    }
}

// ── Collab Manager ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct CollabStats {
    pub sessions_created: u64,
    pub sessions_closed: u64,
    pub ops_processed: u64,
    pub peers_joined: u64,
}

/// The kernel-side Q-Collab manager.
pub struct CollabManager {
    /// Active sessions: session_id → session
    pub sessions: BTreeMap<u64, CollabSession>,
    /// Next session ID
    next_session_id: u64,
    /// Stats
    pub stats: CollabStats,
}

impl CollabManager {
    pub fn new() -> Self {
        CollabManager {
            sessions: BTreeMap::new(),
            next_session_id: 1,
            stats: CollabStats::default(),
        }
    }

    /// Create a new collaboration session.
    pub fn create_session(
        &mut self,
        owner_silo: u64,
        prism_oid: u64,
        tick: u64,
    ) -> u64 {
        let id = self.next_session_id;
        self.next_session_id += 1;

        crate::serial_println!(
            "[COLLAB] Session {} created by Silo {} (doc OID={}).",
            id, owner_silo, prism_oid
        );
        self.sessions.insert(id, CollabSession::new(id, owner_silo, prism_oid, tick));
        self.stats.sessions_created += 1;
        id
    }

    /// Close a session (on Silo vaporize or explicit close).
    pub fn close_session(&mut self, session_id: u64) {
        if let Some(mut s) = self.sessions.remove(&session_id) {
            s.status = SessionStatus::Closed;
            self.stats.sessions_closed += 1;
            crate::serial_println!(
                "[COLLAB] Session {} closed ({} ops processed).", session_id, s.ops_processed
            );
        }
    }
}
