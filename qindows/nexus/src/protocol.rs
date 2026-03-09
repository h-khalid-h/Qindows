//! # Q-Fabric Network Protocol
//!
//! The transport layer for the Global Mesh.
//! Uses QUIC as base with custom extensions for:
//! - Encrypted fiber migration
//! - Object smearing (data replication)
//! - Planetary antibody propagation (threat response < 300ms)
//! - Q-Credits settlement

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

/// Q-Fabric packet types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Peer discovery beacon
    Beacon = 0x01,
    /// Handshake (key exchange)
    Handshake = 0x02,
    /// Data transfer (Prism objects)
    Data = 0x10,
    /// Fiber migration (serialized task)
    FiberMigrate = 0x11,
    /// Object smear (replicate to N peers)
    ObjectSmear = 0x12,
    /// Mesh heartbeat (availability update)
    Heartbeat = 0x20,
    /// Antibody propagation (malware signature)
    Antibody = 0x30,
    /// Q-Credits settlement
    CreditSettlement = 0x40,
    /// TLB shootdown broadcast (distributed page table sync)
    TlbSync = 0x50,
    /// Disconnect notification
    Disconnect = 0xFF,
}

/// A Q-Fabric packet header.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct PacketHeader {
    /// Magic: "QFAB"
    pub magic: [u8; 4],
    /// Protocol version
    pub version: u8,
    /// Packet type
    pub packet_type: u8,
    /// Flags
    pub flags: u16,
    /// Payload length
    pub payload_len: u32,
    /// Source node ID (first 8 bytes of 32-byte node ID)
    pub source_id: u64,
    /// Destination node ID (0 = broadcast)
    pub dest_id: u64,
    /// Sequence number
    pub seq: u64,
    /// Timestamp (microseconds since epoch)
    pub timestamp: u64,
}

impl PacketHeader {
    pub fn new(ptype: PacketType, source: u64, dest: u64) -> Self {
        static SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        PacketHeader {
            magic: *b"QFAB",
            version: 1,
            packet_type: ptype as u8,
            flags: 0,
            payload_len: 0,
            source_id: source,
            dest_id: dest,
            seq: SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            timestamp: 0, // Filled by transport
        }
    }
}

/// Antibody — a threat signature broadcast across the mesh.
///
/// When the Sentinel on any node detects a new threat,
/// it broadcasts an Antibody packet. Within 300ms, every
/// Qindows node in the world knows about it.
#[derive(Debug, Clone)]
pub struct Antibody {
    /// SHA-256 hash of the malicious binary
    pub threat_hash: [u8; 32],
    /// Threat severity (1-10)
    pub severity: u8,
    /// Human-readable description
    pub description: String,
    /// Node that first detected this threat
    pub reporter_id: [u8; 32],
    /// Detection timestamp
    pub detected_at: u64,
    /// Recommended action
    pub action: ThreatAction,
    /// Number of hops this antibody has traveled
    pub hop_count: u16,
}

/// Threat response actions
#[derive(Debug, Clone, Copy)]
pub enum ThreatAction {
    /// Block this binary from loading
    Block,
    /// Quarantine (freeze existing instances)
    Quarantine,
    /// Vaporize (kill all instances immediately)
    Vaporize,
    /// Monitor only (log but don't act)
    Monitor,
}

/// Connection state for a peer in the mesh.
#[derive(Debug)]
pub struct PeerConnection {
    /// Peer's node ID
    pub node_id: [u8; 32],
    /// Connection state
    pub state: ConnectionState,
    /// Latency in microseconds
    pub latency_us: u64,
    /// Shared encryption key (post-handshake)
    pub session_key: [u8; 32],
    /// Packets sent to this peer
    pub packets_sent: u64,
    /// Packets received from this peer
    pub packets_recv: u64,
    /// Bytes transferred
    pub bytes_transferred: u64,
    /// Last heartbeat timestamp
    pub last_heartbeat: u64,
}

/// Peer connection states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Discovered via beacon
    Discovered,
    /// Handshake in progress
    Connecting,
    /// Fully connected and authenticated
    Connected,
    /// Connection degraded (high latency/packet loss)
    Degraded,
    /// Disconnected
    Disconnected,
}

/// The Q-Fabric protocol engine.
pub struct QFabric {
    /// Active peer connections
    pub peers: Vec<PeerConnection>,
    /// Our node ID
    pub local_id: [u8; 32],
    /// Active antibodies (threat signatures)
    pub antibodies: Vec<Antibody>,
    /// Total data transferred (bytes)
    pub total_bytes: u64,
}

impl QFabric {
    pub fn new(local_id: [u8; 32]) -> Self {
        QFabric {
            peers: Vec::new(),
            local_id,
            antibodies: Vec::new(),
            total_bytes: 0,
        }
    }

    /// Broadcast a beacon to discover peers.
    pub fn send_beacon(&self) -> PacketHeader {
        let source = u64::from_le_bytes(self.local_id[..8].try_into().unwrap_or([0; 8]));
        PacketHeader::new(PacketType::Beacon, source, 0) // dest=0 = broadcast
    }

    /// Process a received antibody and propagate to all connected peers (Fix #15).
    ///
    /// When a new threat signature arrives, this function:
    /// 1. Checks deduplication (don't re-process known threats)
    /// 2. Increments hop_count to prevent infinite propagation (max 32 hops)
    /// 3. Stores the antibody locally
    /// 4. Creates relay packets for every Connected peer (real broadcast)
    pub fn process_antibody(&mut self, mut antibody: Antibody) -> Vec<PacketHeader> {
        let mut relay_packets = Vec::new();

        // Check if we already know about this threat
        let already_known = self.antibodies.iter().any(|a| a.threat_hash == antibody.threat_hash);

        if !already_known && antibody.hop_count < 32 {
            antibody.hop_count += 1;

            // Build relay packets for every connected peer
            let source_id = u64::from_le_bytes(
                self.local_id[..8].try_into().unwrap_or([0; 8])
            );

            for peer in &mut self.peers {
                if peer.state == ConnectionState::Connected {
                    let dest_id = u64::from_le_bytes(
                        peer.node_id[..8].try_into().unwrap_or([0; 8])
                    );
                    let mut pkt = PacketHeader::new(PacketType::Antibody, source_id, dest_id);
                    pkt.payload_len = 32 + 1 + 8; // hash + severity + timestamp
                    peer.packets_sent += 1;
                    peer.bytes_transferred += pkt.payload_len as u64;
                    self.total_bytes += pkt.payload_len as u64;
                    relay_packets.push(pkt);
                }
            }

            self.antibodies.push(antibody);
        }

        relay_packets
    }

    /// Check if a binary hash matches any known antibody.
    pub fn is_threat(&self, hash: &[u8; 32]) -> Option<&Antibody> {
        self.antibodies.iter().find(|a| &a.threat_hash == hash)
    }

    /// Get the number of active (connected) peers.
    pub fn active_peers(&self) -> usize {
        self.peers.iter().filter(|p| p.state == ConnectionState::Connected).count()
    }
}
