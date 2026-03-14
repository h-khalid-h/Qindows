//! # Nexus Peer Tier Cap Bridge (Phase 267)
//!
//! ## Architecture Guardian: The Gap
//! `nexus.rs` implements `PeerNode`:
//! - `NodeTier` — Local, Lan, Regional, Global
//! - `PeerNode { node_id, tier, ... }`
//!
//! **Missing link**: Mesh task scheduling to Global-tier nodes had
//! no Admin:EXEC gate. Routing heavy compute tasks to intercontinental
//! peers (>50ms RTT) was done without capability check, degrading
//! local performance (Law 4 DoS).
//!
//! This module provides `NexusPeerTierCapBridge`:
//! Admin:EXEC cap required to route tasks to Global NodeTier.

extern crate alloc;

use crate::nexus::NodeTier;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct PeerTierCapStats {
    pub routes_allowed: u64,
    pub routes_denied:  u64,
}

pub struct NexusPeerTierCapBridge {
    pub stats: PeerTierCapStats,
}

impl NexusPeerTierCapBridge {
    pub fn new() -> Self {
        NexusPeerTierCapBridge { stats: PeerTierCapStats::default() }
    }

    /// Authorize routing to a peer — Global tier requires Admin:EXEC.
    pub fn authorize_route(
        &mut self,
        silo_id: u64,
        target_tier: &NodeTier,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let needs_cap = matches!(target_tier, NodeTier::Global);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.routes_denied += 1;
            crate::serial_println!(
                "[NEXUS] Silo {} route to Global tier denied — Admin:EXEC required", silo_id
            );
            return false;
        }
        self.stats.routes_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PeerTierCapBridge: allowed={} denied={}",
            self.stats.routes_allowed, self.stats.routes_denied
        );
    }
}
