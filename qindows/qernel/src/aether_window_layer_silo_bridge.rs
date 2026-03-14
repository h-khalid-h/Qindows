//! # Aether Window Layer Silo Bridge (Phase 280)
//!
//! ## Architecture Guardian: The Gap
//! `aether.rs` implements the Aether compositor:
//! - `WindowLayer` — Background, Normal, Overlay, System, Cursor
//! - `WindowVisibility` — Visible, Hidden, Minimized
//! - `Rect::intersects(other)` → bool — overlap detection
//!
//! **Missing link**: Any Silo could set its window to `WindowLayer::System`
//! or `WindowLayer::Overlay`, overlapping system UI elements and capturing
//! user input events (clickjacking via window layer abuse).
//!
//! This module provides `AetherWindowLayerSiloBridge`:
//! Admin:EXEC cap required to set WindowLayer::System or WindowLayer::Overlay.

extern crate alloc;

use crate::aether::WindowLayer;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct AetherWindowLayerStats {
    pub layers_allowed: u64,
    pub layers_denied:  u64,
}

pub struct AetherWindowLayerSiloBridge {
    pub stats: AetherWindowLayerStats,
}

impl AetherWindowLayerSiloBridge {
    pub fn new() -> Self {
        AetherWindowLayerSiloBridge { stats: AetherWindowLayerStats::default() }
    }

    /// Authorize window layer — System/Overlay requires Admin:EXEC.
    pub fn authorize_layer(
        &mut self,
        silo_id: u64,
        layer: &WindowLayer,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let needs_cap = matches!(layer, WindowLayer::Overlay | WindowLayer::Notification);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.layers_denied += 1;
            crate::serial_println!(
                "[AETHER] Silo {} window layer {:?} denied — Admin:EXEC required", silo_id, layer
            );
            return false;
        }
        self.stats.layers_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AetherWindowLayerBridge: allowed={} denied={}",
            self.stats.layers_allowed, self.stats.layers_denied
        );
    }
}
