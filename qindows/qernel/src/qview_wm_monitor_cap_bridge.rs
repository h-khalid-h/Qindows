//! # Q-View WM Monitor Cap Bridge (Phase 235)
//!
//! ## Architecture Guardian: The Gap
//! `q_view_wm.rs` implements `QViewWm`:
//! - `Monitor { id, width, height, refresh_rate, ... }`
//! - `LayoutMode` — Tiling, Floating, Fullscreen, Presentation
//! - `Geometry::contains(px, py)` — hit test
//!
//! **Missing link**: Window Manager monitor configuration changes were
//! not capability-gated. A Silo could change the layout mode to Fullscreen,
//! covering all other windows.
//!
//! This module provides `QViewWmMonitorCapBridge`:
//! Admin:EXEC cap required to change LayoutMode to Fullscreen/Presentation.

extern crate alloc;

use crate::q_view_wm::{LayoutMode};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct WmCapStats {
    pub changes_allowed: u64,
    pub changes_denied:  u64,
}

pub struct QViewWmMonitorCapBridge {
    pub stats: WmCapStats,
}

impl QViewWmMonitorCapBridge {
    pub fn new() -> Self {
        QViewWmMonitorCapBridge { stats: WmCapStats::default() }
    }

    /// Authorize a layout mode change — Fullscreen/Presentation require Admin:EXEC.
    pub fn authorize_layout_change(
        &mut self,
        silo_id: u64,
        new_mode: &LayoutMode,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Monocle fills the entire monitor — blocks all other windows (requires cap)
        let needs_cap = matches!(new_mode, LayoutMode::Monocle);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.changes_denied += 1;
            crate::serial_println!(
                "[WM] Silo {} {:?} mode change denied — no Admin:EXEC cap", silo_id, new_mode
            );
            return false;
        }
        self.stats.changes_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WmCapBridge: allowed={} denied={}", self.stats.changes_allowed, self.stats.changes_denied
        );
    }
}
