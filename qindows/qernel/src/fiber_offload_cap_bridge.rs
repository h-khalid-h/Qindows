//! # Fiber Offload Cap Bridge (Phase 163)
//!
//! ## Architecture Guardian: The Gap
//! `fiber_offload.rs` implements `FiberOffloadEngine`:
//! - `initiate_offload(silo_id, fiber_id, ...)` — sends fiber to remote node
//! - `recall(silo_id, fiber_id, tick)` — pulls fiber back
//! - `on_remote_ack(silo_id, fiber_id, tick)` → bool
//!
//! **Missing link**: Any Silo could initiate a remote fiber offload to any
//! Nexus peer without capability check. This violated Law 4 (no silo moves
//! itself to a different execution context without explicit permission) and
//! potentially enabled computation laundering across the mesh.
//!
//! This module provides `FiberOffloadCapBridge`:
//! 1. `offload_with_cap_check()` — Network:EXEC required for cross-node offload
//! 2. `recall_with_cap_check()` — Network:READ required to pull back

extern crate alloc;

use crate::fiber_offload::FiberOffloadEngine;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC, CAP_READ};

#[derive(Debug, Default, Clone)]
pub struct FiberOffloadBridgeStats {
    pub offloads_allowed: u64,
    pub offloads_denied:  u64,
    pub recalls_allowed:  u64,
    pub recalls_denied:   u64,
}

pub struct FiberOffloadCapBridge {
    pub engine: FiberOffloadEngine,
    pub stats:  FiberOffloadBridgeStats,
}

impl FiberOffloadCapBridge {
    pub fn new() -> Self {
        FiberOffloadCapBridge {
            engine: FiberOffloadEngine::new(),
            stats:  FiberOffloadBridgeStats::default(),
        }
    }

    /// Initiate a fiber offload to a remote Nexus node.
    /// Requires Network:EXEC cap (cross-node execution = network capability).
    pub fn offload_with_cap_check(
        &mut self,
        silo_id: u64,
        fiber_id: u64,
        target_node: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Network, CAP_EXEC, 0, tick) {
            self.stats.offloads_denied += 1;
            crate::serial_println!(
                "[FIBER OFFLOAD] Silo {} denied offload → node {} (no Network:EXEC, Law 4)",
                silo_id, target_node
            );
            return false;
        }
        self.stats.offloads_allowed += 1;
        match self.engine.initiate_offload(silo_id, fiber_id, target_node, tick) {
            Ok(()) => true,
            Err(e) => {
                crate::serial_println!("[FIBER OFFLOAD] initiate_offload failed: {}", e);
                false
            }
        }
    }

    /// Recall a fiber from a remote node. Requires Network:READ cap.
    pub fn recall_with_cap_check(
        &mut self,
        silo_id: u64,
        fiber_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Network, CAP_READ, 0, tick) {
            self.stats.recalls_denied += 1;
            crate::serial_println!(
                "[FIBER OFFLOAD] Silo {} denied recall (no Network:READ)", silo_id
            );
            return false;
        }
        self.stats.recalls_allowed += 1;
        self.engine.recall(silo_id, fiber_id, tick);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FiberOffloadBridge: offloads={}/{} recalls={}/{}",
            self.stats.offloads_allowed, self.stats.offloads_denied,
            self.stats.recalls_allowed, self.stats.recalls_denied
        );
    }
}
