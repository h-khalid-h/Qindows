//! # Nexus Kernel Bridge (Phase 111)
//!
//! ## Architecture Guardian: The Gap
//! - `nexus/` crate: 46 modules implementing the full mesh networking stack
//!   (QUIC, DHT, mDNS, gossip, consensus, relay, VPN, etc.)
//! - `qernel/src/qfabric.rs` (Phase 55): kernel-side QUIC transport
//! - `qernel/src/nexus.rs` (Phase 61): kernel-side Genesis/mesh core
//!
//! **Missing link**: The `nexus/` crate runs as a **Nexus Silo** (SILO_ID=5).
//! The kernel's `qfabric.rs` handled the raw QUIC byte stream, but never
//! connected to the mesh routing decisions made by `nexus/src/mesh_routing.rs`.
//!
//! This bridge:
//! 1. Routes outgoing Q-Fabric packets to the Nexus Silo for mesh routing
//! 2. Receives incoming packets from the Nexus Silo and delivers to target Silo
//! 3. Handles Law 7 traffic accounting for all mesh packets
//! 4. Provides the kernel API that `nexus/src/qfabric.rs` calls via IPC
//!
//! ## Nexus Silo IPC Protocol
//! The Nexus Silo communicates with the kernel via Q-Ring opcodes:
//! - `FabricSend` → kernel → Nexus Silo (routes `NetSend` to correct peer)
//! - `FabricRecv` → Nexus Silo → kernel → destination Silo (delivers inbound)
//! - `NetSend` direct → Law 7 cap check → qfabric → QUIC socket

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use crate::qring_async::{QRingProcessor, SqEntry, SqOpcode};

// ── Nexus Silo ID ─────────────────────────────────────────────────────────────

/// Fixed canonical Silo ID for the Nexus mesh engine.
pub const NEXUS_SILO_ID: u64 = 5;
/// Fixed canonical Silo ID for the Aether compositor.
pub const AETHER_SILO_ID: u64 = 3;
/// Fixed canonical Silo ID for Q-Shell.
pub const SHELL_SILO_ID: u64 = 2;
/// Fixed canonical Silo ID for Synapse.
pub const SYNAPSE_SILO_ID: u64 = 4;
/// Fixed canonical Silo ID for Prism.
pub const PRISM_SILO_ID: u64 = 6;

// ── Packet Routing Table ──────────────────────────────────────────────────────

/// A routing entry — maps destination NodeId prefix to a Nexus hop.
#[derive(Debug, Clone, Copy)]
pub struct NexusRoute {
    /// First 8 bytes of destination NodeId (prefix match)
    pub dest_prefix: u64,
    /// Next-hop NodeId (first 8 bytes)
    pub next_hop: u64,
    /// Latency estimate in ticks
    pub latency_ticks: u32,
    /// Route age in ticks
    pub age_ticks: u32,
    /// Whether this is a direct/single-hop connection
    pub direct: bool,
}

// ── Traffic Accounting ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct NexusBridgeStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub route_lookups: u64,
    pub route_misses: u64,   // DHT lookup needed
    pub law7_denials: u64,   // No NET_SEND cap
    pub silo_deliveries: u64,
}

// ── Nexus Kernel Bridge ───────────────────────────────────────────────────────

/// Kernel-side interface between qfabric.rs QUIC transport and the Nexus Silo.
pub struct NexusKernelBridge {
    /// Simple routing table — populated by Nexus Silo via IPC
    pub routes: Vec<NexusRoute>,
    pub stats: NexusBridgeStats,
    pub self_node_id: [u8; 32],
}

impl NexusKernelBridge {
    pub fn new(self_node_id: [u8; 32]) -> Self {
        NexusKernelBridge {
            routes: Vec::new(),
            stats: NexusBridgeStats::default(),
            self_node_id,
        }
    }

    /// Route an outgoing packet from a Silo to the mesh.
    /// Called by qring_dispatch.rs when FabricSend opcode arrives.
    pub fn send_packet(
        &mut self,
        from_silo: u64,
        dest_node_prefix: u64,
        payload_len: u32,
        qring: &mut QRingProcessor,
        tick: u64,
    ) -> bool {
        self.stats.route_lookups += 1;

        // 1. Law 7: verify NET_SEND cap (in production: cap_token::check)
        // For now: we trust it passed qring_dispatch's cap check
        self.stats.bytes_sent += payload_len as u64;
        self.stats.packets_sent += 1;

        // 2. Look up route
        let next_hop = self.route_lookup(dest_node_prefix);

        if next_hop == 0 {
            // Route miss — ask Nexus Silo to do DHT lookup
            self.stats.route_misses += 1;
            let sqe = SqEntry {
                opcode: SqOpcode::FabricSend as u16,
                flags: 1, // ROUTE_MISS flag
                user_data: tick,
                addr: dest_node_prefix,
                len: payload_len,
                aux: from_silo as u32,
            };
            self.inject_nexus(sqe, qring, tick);
            crate::serial_println!(
                "[NEXUS BRIDGE] Route miss: dest={:#x}, forwarding to Nexus Silo for DHT", dest_node_prefix
            );
            return false; // deferred
        }

        // 3. Direct route known — submit to Q-Fabric
        let sqe = SqEntry {
            opcode: SqOpcode::FabricSend as u16,
            flags: 0,
            user_data: tick,
            addr: next_hop,
            len: payload_len,
            aux: from_silo as u32,
        };
        self.inject_nexus(sqe, qring, tick);
        crate::serial_println!(
            "[NEXUS BRIDGE] FabricSend: from_silo={} → next_hop={:#x} len={}", from_silo, next_hop, payload_len
        );
        true
    }

    /// Deliver an inbound packet from Nexus Silo to a local destination Silo.
    pub fn deliver_inbound(
        &mut self,
        dest_silo: u64,
        src_node_prefix: u64,
        payload_len: u32,
        qring: &mut QRingProcessor,
        tick: u64,
    ) {
        self.stats.packets_received += 1;
        self.stats.bytes_received += payload_len as u64;
        self.stats.silo_deliveries += 1;

        // Deliver via IpcSend to destination Silo's Q-Ring
        let sqe = SqEntry {
            opcode: SqOpcode::IpcSend as u16,
            flags: 0,
            user_data: tick,
            addr: src_node_prefix,
            len: payload_len,
            aux: 0xFAB1, // Q-Fabric inbound channel
        };

        if !qring.rings.contains_key(&dest_silo) {
            qring.register_silo(dest_silo);
        }
        if let Some(ring) = qring.rings.get_mut(&dest_silo) {
            if ring.submit(sqe) {
                crate::serial_println!(
                    "[NEXUS BRIDGE] Inbound delivered: src={:#x} → Silo {} len={}",
                    src_node_prefix, dest_silo, payload_len
                );
            }
        }
    }

    /// Install a route (called by Nexus Silo when it learns about a peer).
    pub fn install_route(&mut self, route: NexusRoute) {
        // Replace existing entry if dest_prefix already known
        if let Some(existing) = self.routes.iter_mut().find(|r| r.dest_prefix == route.dest_prefix) {
            *existing = route;
        } else {
            self.routes.push(route);
        }
    }

    fn route_lookup(&self, dest_prefix: u64) -> u64 {
        self.routes.iter()
            .find(|r| r.dest_prefix == dest_prefix)
            .map(|r| r.next_hop)
            .unwrap_or(0)
    }

    fn inject_nexus(&self, sqe: SqEntry, qring: &mut QRingProcessor, _tick: u64) {
        if !qring.rings.contains_key(&NEXUS_SILO_ID) {
            qring.register_silo(NEXUS_SILO_ID);
        }
        if let Some(ring) = qring.rings.get_mut(&NEXUS_SILO_ID) {
            ring.submit(sqe);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NexusBridge: sent={}pkt/{} bytes recv={}pkt/{} bytes routes={} misses={}",
            self.stats.packets_sent, self.stats.bytes_sent,
            self.stats.packets_received, self.stats.bytes_received,
            self.routes.len(), self.stats.route_misses
        );
    }
}
