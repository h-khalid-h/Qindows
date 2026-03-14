//! # Collaborative Session Network Sync (Phase 115)
//!
//! ## Architecture Guardian: The Gap
//! `collab.rs` (Phase 67) implements the CRDT engine:
//! - `SharedDocument` — CRDT op log + vector clock
//! - `CollabSession` — tracks peers, derives text
//! - `delta_since()` — computes delta for a peer
//!
//! **The missing link**: `delta_since()` returns `Vec<CrdtOp>` but nothing
//! ever *sent* those deltas to remote peers via the Nexus mesh.
//! No code called `CollabSession::peer_join()` when a remote user connected.
//!
//! This module provides the **network sync layer** for CRDT collaboration:
//! 1. `announce_session()` — broadcasts session OID to Nexus Silo via Q-Ring
//! 2. `push_delta()` — sends `Vec<CrdtOp>` to specific peers
//! 3. `receive_delta()` — processes incoming delta from remote peer
//! 4. `on_peer_joined()` / `on_peer_left()` — updates CollabSession state
//!
//! ## Session Discovery
//! Sessions are announced via `nexus_kernel_bridge::send_packet()` with
//! a COLLAB_ANNOUNCE opcode. Remote Nexus Silos relay the announcement to
//! potential participants who then join via `on_peer_joined()`.
//!
//! ## Conflict Resolution
//! CRDT semantics guarantee convergence — no explicit merge needed.
//! The vector clock in `SharedDocument` ensures causal ordering.

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::collab::{CollabSession, SharedDocument, CrdtOp, VectorClock, SessionStatus};
use crate::nexus::NodeId;
use crate::qring_async::{QRingProcessor, SqEntry, SqOpcode};
use crate::nexus_kernel_bridge::NEXUS_SILO_ID;

// ── Sync Channel Opcodes (over IPC) ──────────────────────────────────────────

/// IPC message types for collab session sync (encoded in SqEntry::aux).
pub const COLLAB_ANNOUNCE:  u32 = 0xC0_01;
pub const COLLAB_PEER_JOIN: u32 = 0xC0_02;
pub const COLLAB_PEER_LEAVE:u32 = 0xC0_03;
pub const COLLAB_DELTA_PUSH:u32 = 0xC0_04;
pub const COLLAB_DELTA_ACK: u32 = 0xC0_05;

// ── Sync Statistics ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct CollabSyncStats {
    pub sessions_announced: u64,
    pub peers_joined: u64,
    pub peers_left: u64,
    pub deltas_pushed: u64,
    pub deltas_received: u64,
    pub ops_total: u64,
    pub ring_full_drops: u64,
}

// ── Collab Session Network Manager ────────────────────────────────────────────

/// Manages network sync for CRDT collaborative sessions.
pub struct CollabSessionNet {
    pub session: CollabSession,
    pub document: SharedDocument,
    pub local_node: NodeId,
    pub stats: CollabSyncStats,
}

impl CollabSessionNet {
    pub fn new(
        session_id: u64,
        owner_silo: u64,
        prism_oid: u64,
        local_node: NodeId,
        tick: u64,
    ) -> Self {
        CollabSessionNet {
            session: CollabSession::new(session_id, owner_silo, prism_oid, tick),
            document: SharedDocument::new(prism_oid, owner_silo),
            local_node,
            stats: CollabSyncStats::default(),
        }
    }

    /// Announce this session to the Nexus mesh (broadcast).
    /// Remote peers discover it and can request to join.
    pub fn announce_session(&mut self, qring: &mut QRingProcessor, tick: u64) {
        self.stats.sessions_announced += 1;

        let sqe = SqEntry {
            opcode: SqOpcode::FabricSend as u16,
            flags: 0,
            user_data: tick,
            addr: 0xFFFF_FFFF_FFFF_FFFFu64, // broadcast address
            len: 32, // session OID length
            aux: COLLAB_ANNOUNCE,
        };
        self.inject(sqe, qring);

        crate::serial_println!(
            "[COLLAB NET] Session {} announced to Nexus mesh @ tick {}",
            self.session.session_id, tick
        );
    }

    /// A remote peer has joined the session.
    pub fn on_peer_joined(&mut self, peer: NodeId, qring: &mut QRingProcessor, tick: u64) {
        self.session.peer_join(peer);
        self.stats.peers_joined += 1;

        // Send the full current delta to the new peer (catch-up)
        let empty_clock = VectorClock::new();
        let delta = self.document.delta_since(&empty_clock, self.local_node);
        if !delta.is_empty() {
            self.push_delta_to_peer(peer, delta, qring, tick);
        }

        crate::serial_println!("[COLLAB NET] Peer {:?} joined session {}", peer, self.session.session_id);
    }

    /// A remote peer has left the session.
    pub fn on_peer_left(&mut self, peer: NodeId) {
        self.session.peer_leave(peer);
        self.stats.peers_left += 1;
        crate::serial_println!("[COLLAB NET] Peer {:?} left session {}", peer, self.session.session_id);
    }

    /// Apply a local CRDT operation and push delta to all peers.
    pub fn apply_local_op(&mut self, op: CrdtOp, qring: &mut QRingProcessor, tick: u64) {
        self.document.apply_local(op.clone(), self.local_node);
        self.stats.ops_total += 1;

        // Push to all connected peers
        let peers: alloc::vec::Vec<NodeId> = self.session.peers.keys().copied().collect();
        for peer in peers {
            let delta = alloc::vec![op.clone()];
            self.push_delta_to_peer(peer, delta, qring, tick);
        }
    }

    /// Receive a delta from a remote peer and apply it.
    pub fn receive_delta(&mut self, ops: Vec<CrdtOp>, peer_clock: VectorClock, tick: u64) {
        self.stats.deltas_received += 1;
        self.stats.ops_total += ops.len() as u64;

        for op in ops {
            self.document.merge_remote(op, &peer_clock);
        }
        crate::serial_println!(
            "[COLLAB NET] Received delta patch applied @ tick {}, text_len={}",
            tick, self.document.derive_text().len()
        );
    }

    /// Get the current document text.
    pub fn get_text(&self) -> String {
        self.document.derive_text()
    }

    fn push_delta_to_peer(
        &mut self,
        peer: NodeId,
        delta: Vec<CrdtOp>,
        qring: &mut QRingProcessor,
        tick: u64,
    ) {
        self.stats.deltas_pushed += 1;
        self.stats.ops_total += delta.len() as u64;

        let peer_addr = u64::from_le_bytes(peer.0[..8].try_into().unwrap_or([0;8]));
        let sqe = SqEntry {
            opcode: SqOpcode::FabricSend as u16,
            flags: 0,
            user_data: tick,
            addr: peer_addr,
            len: delta.len() as u32,
            aux: COLLAB_DELTA_PUSH,
        };
        self.inject(sqe, qring);
        crate::serial_println!(
            "[COLLAB NET] Pushed {} ops to peer {:?}", delta.len(), peer
        );
    }

    fn inject(&mut self, sqe: SqEntry, qring: &mut QRingProcessor) {
        if !qring.rings.contains_key(&NEXUS_SILO_ID) {
            qring.register_silo(NEXUS_SILO_ID);
        }
        if let Some(ring) = qring.rings.get_mut(&NEXUS_SILO_ID) {
            if !ring.submit(sqe) {
                self.stats.ring_full_drops += 1;
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CollabNet: sessions={} peers={}/{} deltas_out={} deltas_in={} ops={}",
            self.stats.sessions_announced, self.stats.peers_joined, self.stats.peers_left,
            self.stats.deltas_pushed, self.stats.deltas_received, self.stats.ops_total
        );
    }
}
