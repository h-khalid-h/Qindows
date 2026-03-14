//! # Q-Fabric — QUIC-Native Network Transport (Phase 55)
//!
//! Q-Fabric is Qindows's zero-handshake, multi-path network layer.
//! It sits above the NIC driver (UMDF) and below the Universal Namespace (UNS),
//! providing QUIC-style semantics for all process and node communication.
//!
//! ## Why QUIC?
//! - **0-RTT connections**: Cached session tickets allow instant reconnection
//! - **Multi-path**: Simultaneous use of Wi-Fi + Ethernet + cellular
//! - **Stream multiplexing**: 1000s of logical streams over one UDP socket
//! - **Built-in encryption**: TLS 1.3 mandatory (no cleartext Q-Fabric traffic)
//!
//! ## Q-Manifest Law 9: Universal Namespace
//! All communication — local IPC, LAN, WAN — uses Q-Fabric addressing.
//! A Q-Fabric address `qfa://node-id/silo-id/service` is location-transparent:
//! the kernel routes it optimally (shared memory → LAN → WAN).
//!
//! ## Architecture Guardian Note
//! Q-Fabric is a **user-mode service** (runs in its own Silo) that uses the
//! kernel's IPC layer (Q-Ring) for node-local delivery and raw UDP sockets
//! for remote delivery. This module is the KERNEL INTERFACE to Q-Fabric:
//! the syscall stubs and packet routing tables. The QUIC state machine lives
//! in the Q-Fabric userland Silo.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── Q-Fabric Addressing ───────────────────────────────────────────────────────

/// A 128-bit Q-Fabric node identifier (globally unique).
/// Derived from the system's public key fingerprint at genesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct QFabricNodeId(pub [u8; 16]);

impl QFabricNodeId {
    /// The local node's ID (placeholder — set at boot from genesis key).
    pub const LOCAL: Self = QFabricNodeId([0u8; 16]);

    pub fn is_local(&self) -> bool { *self == Self::LOCAL }
}

/// A stream ID within a Q-Fabric connection.
pub type StreamId = u32;

/// Delivery priority for a Q-Fabric packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryPriority {
    /// Best-effort background (telemetry, mesh sync)
    Background = 0,
    /// Normal application data
    Normal = 1,
    /// Real-time (audio, video, BCI signals)
    Realtime = 2,
    /// Control plane (capability negotiation, Sentinel alerts)
    Control = 3,
}

// ── Q-Fabric Packet ───────────────────────────────────────────────────────────

/// Maximum payload size for a single Q-Fabric packet (jumbo frame safe).
pub const QFABRIC_MAX_PAYLOAD: usize = 65507; // Max UDP payload

/// A Q-Fabric network packet.
#[derive(Debug, Clone)]
pub struct QFabricPacket {
    /// Destination node
    pub dest_node: QFabricNodeId,
    /// Destination Silo (Ring-3 service endpoint)
    pub dest_silo: u64,
    /// Stream within the connection
    pub stream_id: StreamId,
    /// Packet sequence number (for ordering + loss detection)
    pub seq: u64,
    /// Delivery priority
    pub priority: DeliveryPriority,
    /// Payload bytes (application data)
    pub payload: Vec<u8>,
    /// Is this a 0-RTT cached packet?
    pub is_0rtt: bool,
}

// ── Q-Fabric Route Table ──────────────────────────────────────────────────────

/// How to reach a remote node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    /// Same machine — route via Q-Ring IPC (zero-copy)
    LocalIpc,
    /// Same LAN — route via Ethernet/Wi-Fi
    LanUdp,
    /// Remote — route via WAN (Internet)
    WanUdp,
    /// Via relay (NAT traversal or mesh relay node)
    Relay,
}

/// A single route table entry.
#[derive(Debug, Clone)]
pub struct QFabricRoute {
    pub node: QFabricNodeId,
    pub kind: RouteKind,
    /// IPv6 address of the remote node (128 bits)
    pub ipv6_addr: [u8; 16],
    /// UDP port
    pub port: u16,
    /// Round-trip time estimate (microseconds)
    pub rtt_us: u32,
    /// Is this route currently active?
    pub active: bool,
    /// Packets forwarded via this route.
    pub packets_forwarded: u64,
}

// ── Q-Fabric Connection State ─────────────────────────────────────────────────

/// Connection state (QUIC-inspired).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Dialing (0-RTT attempt in flight)
    Connecting,
    /// Fully established (TLS 1.3 handshake complete)
    Established,
    /// Draining (graceful close)
    Draining,
    /// Closed
    Closed,
}

/// A Q-Fabric connection to one remote node.
#[derive(Debug, Clone)]
pub struct QFabricConnection {
    pub remote_node: QFabricNodeId,
    pub state: ConnectionState,
    /// Active streams on this connection
    pub streams: BTreeMap<StreamId, QFabricStream>,
    /// Bytes sent on this connection
    pub bytes_sent: u64,
    /// Bytes received on this connection
    pub bytes_recv: u64,
    /// 0-RTT session ticket (cached from previous connection)
    pub session_ticket: Option<[u8; 32]>,
    /// Next stream ID to allocate
    next_stream_id: StreamId,
}

impl QFabricConnection {
    pub fn new(remote_node: QFabricNodeId) -> Self {
        QFabricConnection {
            remote_node,
            state: ConnectionState::Connecting,
            streams: BTreeMap::new(),
            bytes_sent: 0,
            bytes_recv: 0,
            session_ticket: None,
            next_stream_id: 1,
        }
    }

    /// Open a new logical stream on this connection.
    pub fn open_stream(&mut self, priority: DeliveryPriority) -> StreamId {
        let sid = self.next_stream_id;
        self.next_stream_id += 1;
        self.streams.insert(sid, QFabricStream::new(sid, priority));
        sid
    }
}

/// A multiplexed logical stream within a Q-Fabric connection.
#[derive(Debug, Clone)]
pub struct QFabricStream {
    pub id: StreamId,
    pub priority: DeliveryPriority,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub packets_lost: u64,
}

impl QFabricStream {
    pub fn new(id: StreamId, priority: DeliveryPriority) -> Self {
        QFabricStream { id, priority, bytes_sent: 0, bytes_recv: 0, packets_lost: 0 }
    }
}

// ── Q-Fabric Router ───────────────────────────────────────────────────────────

/// Statistics for the Q-Fabric router.
#[derive(Debug, Default, Clone)]
pub struct QFabricStats {
    pub packets_sent: u64,
    pub packets_recv: u64,
    pub packets_dropped: u64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub connections_opened: u64,
    pub connections_closed: u64,
    pub zero_rtt_hits: u64,
}

/// The Q-Fabric kernel-side router.
///
/// Owns the route table, active connections, and the send/receive queues.
/// The user-mode Q-Fabric Silo submits packets via `SyscallId::NetSend`
/// and receives packets via `SyscallId::NetRecv` — both route through here.
pub struct QFabricRouter {
    /// Route table: NodeId → RouteEntry
    pub routes: BTreeMap<QFabricNodeId, QFabricRoute>,
    /// Active connections: NodeId → Connection
    pub connections: BTreeMap<QFabricNodeId, QFabricConnection>,
    /// Send queue (packets waiting for transmission)
    pub send_queue: Vec<QFabricPacket>,
    /// Receive queue (packets waiting for the destination Silo)
    pub recv_queue: Vec<QFabricPacket>,
    /// Max queue depth
    pub max_queue_depth: usize,
    /// Stats
    pub stats: QFabricStats,
}

impl QFabricRouter {
    pub fn new() -> Self {
        QFabricRouter {
            routes: BTreeMap::new(),
            connections: BTreeMap::new(),
            send_queue: Vec::new(),
            recv_queue: Vec::new(),
            max_queue_depth: 4096,
            stats: QFabricStats::default(),
        }
    }

    /// Add or update a route to a node.
    pub fn add_route(&mut self, route: QFabricRoute) {
        self.routes.insert(route.node, route);
    }

    /// Open a connection to a remote node (QUIC-style).
    ///
    /// If a cached session ticket exists, attempts 0-RTT immediately.
    pub fn connect(&mut self, node: QFabricNodeId, ticket: Option<[u8; 32]>) -> &mut QFabricConnection {
        let conn = self.connections.entry(node).or_insert_with(|| {
            let mut c = QFabricConnection::new(node);
            if ticket.is_some() {
                c.session_ticket = ticket;
                c.state = ConnectionState::Established; // Optimistic 0-RTT
                self.stats.zero_rtt_hits += 1;
            }
            c
        });
        self.stats.connections_opened += 1;
        conn
    }

    /// Enqueue a packet for transmission.
    ///
    /// Routes to Q-Ring IPC if the destination is local, otherwise queues
    /// for UDP transmission by the NIC driver.
    pub fn send(&mut self, packet: QFabricPacket) -> Result<(), &'static str> {
        if self.send_queue.len() >= self.max_queue_depth {
            self.stats.packets_dropped += 1;
            return Err("Q-Fabric: send queue full");
        }

        let is_local = packet.dest_node.is_local();
        let payload_len = packet.payload.len() as u64;

        self.stats.packets_sent += 1;
        self.stats.bytes_sent += payload_len;

        if is_local {
            // Local delivery: push to recv_queue immediately (simulates Q-Ring)
            crate::serial_println!(
                "[Q-Fabric] Local delivery: Silo {} stream {}",
                packet.dest_silo, packet.stream_id
            );
            self.recv_queue.push(packet);
        } else {
            crate::serial_println!(
                "[Q-Fabric] Remote send: {} bytes to node {:?} stream {}",
                payload_len, packet.dest_node.0, packet.stream_id
            );
            self.send_queue.push(packet);
        }
        Ok(())
    }

    /// Dequeue the next received packet for a specific Silo.
    pub fn recv_for_silo(&mut self, silo_id: u64) -> Option<QFabricPacket> {
        let pos = self.recv_queue.iter().position(|p| p.dest_silo == silo_id)?;
        let pkt = self.recv_queue.remove(pos);
        self.stats.packets_recv += 1;
        self.stats.bytes_recv += pkt.payload.len() as u64;
        Some(pkt)
    }

    /// Flush the send queue to the NIC transmit ring.
    ///
    /// Called periodically by the network interrupt handler.
    /// In production: DMA transfers to NIC TX descriptors.
    pub fn flush_send_queue(&mut self) -> usize {
        let count = self.send_queue.len();
        self.send_queue.clear(); // Simulated: NIC picks up packets
        count
    }
}
