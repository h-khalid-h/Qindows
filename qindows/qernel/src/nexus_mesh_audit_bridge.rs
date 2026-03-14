//! # Nexus Mesh Audit Bridge (Phase 183)
//!
//! ## Architecture Guardian: The Gap
//! `nexus_kernel_bridge.rs` implements `NexusKernelBridge`:
//! - `send_packet(from_silo, dest_node_prefix, payload_len, qring, tick)` — complex
//! - `deliver_inbound(packet, ...)` — complex
//! - `install_route(route: NexusRoute)` — add route table entry
//!
//! **Missing link**: Nexus packet sends were rate-limited within nexus_kernel_bridge
//! only as a production comment — never enforced per Silo. A Silo with Network cap
//! could DoS the mesh by calling send_packet in a tight loop (Law 4 violation).
//!
//! This module provides `NexusMeshAuditBridge`:
//! Per-Silo packet rate limit (64/tick) enforced before forwarding to NexusKernelBridge.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::nexus_kernel_bridge::{NexusKernelBridge, NexusRoute};
use crate::qring_async::QRingProcessor;

const MAX_PACKETS_PER_TICK: u64 = 64;

#[derive(Debug, Default, Clone)]
pub struct NexusAuditStats {
    pub packets_sent:    u64,
    pub packets_dropped: u64,
    pub routes_installed: u64,
}

struct SiloNetState { sent_this_tick: u64 }

pub struct NexusMeshAuditBridge {
    pub bridge:    NexusKernelBridge,
    silo_state:    BTreeMap<u64, SiloNetState>,
    pub stats:     NexusAuditStats,
}

impl NexusMeshAuditBridge {
    pub fn new(self_node_id: [u8; 32]) -> Self {
        NexusMeshAuditBridge {
            bridge: NexusKernelBridge::new(self_node_id),
            silo_state: BTreeMap::new(),
            stats: NexusAuditStats::default(),
        }
    }

    /// Send a Nexus mesh packet — rate-limited per Silo (64/tick).
    pub fn send_with_rate_limit(
        &mut self,
        silo_id: u64,
        dest_node_prefix: u64,
        payload_len: u32,
        qring: &mut QRingProcessor,
        tick: u64,
    ) -> bool {
        let state = self.silo_state.entry(silo_id)
            .or_insert(SiloNetState { sent_this_tick: 0 });
        if state.sent_this_tick >= MAX_PACKETS_PER_TICK {
            self.stats.packets_dropped += 1;
            crate::serial_println!(
                "[NEXUS BRIDGE] Silo {} rate-limited ({}/tick)", silo_id, MAX_PACKETS_PER_TICK
            );
            return false;
        }
        state.sent_this_tick += 1;
        self.stats.packets_sent += 1;
        self.bridge.send_packet(silo_id, dest_node_prefix, payload_len, qring, tick)
    }

    /// Install a mesh routing table entry.
    pub fn install_route(&mut self, route: NexusRoute) {
        self.stats.routes_installed += 1;
        self.bridge.install_route(route);
    }

    pub fn on_tick_reset(&mut self) {
        for s in self.silo_state.values_mut() { s.sent_this_tick = 0; }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NexusBridge: sent={} dropped={} routes={}",
            self.stats.packets_sent, self.stats.packets_dropped, self.stats.routes_installed
        );
    }
}
