//! # Aether Cap Bridge (Phase 146)
//!
//! ## Architecture Guardian: The Gap
//! `aether_kit_bridge.rs` implements `AetherKitBridge`:
//! - `submit_widget_tree(silo_id, root, w, h, kit, qring, tick)` — renders widget tree
//! - `compositor_frame_tick(chimera, qring, tick)` — vsync driver
//!
//! **Missing link**: Widget submission was never gated behind the Aether CapToken.
//! Any Silo could render or blit over other windows, violating Law 3 (UI isolation).
//!
//! This module provides `AetherCapBridge`:
//! 1. `check_aether_cap()` — Aether:EXEC cap check before submission
//! 2. Public stats tracking for all UI submissions

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AetherCapBridgeStats {
    pub frames_allowed: u64,
    pub frames_denied:  u64,
}

// ── Aether Cap Bridge ─────────────────────────────────────────────────────────

/// Gates AetherKitBridge widget submission behind Aether CapToken (Law 3).
pub struct AetherCapBridge {
    pub stats: AetherCapBridgeStats,
}

impl AetherCapBridge {
    pub fn new() -> Self {
        AetherCapBridge { stats: AetherCapBridgeStats::default() }
    }

    /// Check if a Silo is allowed to render (Aether:EXEC cap required).
    /// Callers invoke `AetherKitBridge::submit_widget_tree()` only if this returns true.
    pub fn check_aether_cap(
        &mut self,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Law 3: Silo must hold Aether:EXEC to render UI
        if forge.check(silo_id, CapType::Aether, CAP_EXEC, 0, tick) {
            self.stats.frames_allowed += 1;
            true
        } else {
            self.stats.frames_denied += 1;
            crate::serial_println!(
                "[AETHER CAP] Silo {} denied render — no Aether:EXEC cap (Law 3)", silo_id
            );
            false
        }
    }

    /// Record a compositor vsync tick.
    pub fn tick(&mut self) {
        // Future: rate-limit UI ticks per Silo here
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AetherCapBridge: allowed={} denied={}",
            self.stats.frames_allowed, self.stats.frames_denied
        );
    }
}
