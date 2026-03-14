//! # QView Browser Nav Cap Bridge (Phase 234)
//!
//! ## Architecture Guardian: The Gap
//! `q_view_browser.rs` implements `QViewTab`:
//! - `QViewTab { silo_id, nav_history: Vec<NavEntry>, ... }`
//! - `apply_widget_update(nodes: Vec<QKitNode>)` — update browser DOM
//! - `NavEntry` — URL, title, Silo nav event
//!
//! **Missing link**: `apply_widget_update()` could be called cross-Silo,
//! allowing one Silo to inject DOM nodes into another Silo's browser tab.
//! A keylogger could overlay a transparent input widget on a banking tab.
//!
//! This module provides `QViewBrowserNavCapBridge`:
//! Silo ownership enforcement on `apply_widget_update()`.

extern crate alloc;
use alloc::vec::Vec;

use crate::q_view_browser::{QViewTab, QKitNode};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct BrowserNavCapStats {
    pub updates_allowed: u64,
    pub updates_denied:  u64,
}

pub struct QViewBrowserNavCapBridge {
    pub stats: BrowserNavCapStats,
}

impl QViewBrowserNavCapBridge {
    pub fn new() -> Self {
        QViewBrowserNavCapBridge { stats: BrowserNavCapStats::default() }
    }

    /// Apply widget update — caller must own the tab (Silo ownership).
    pub fn apply_widget_update(
        &mut self,
        tab: &mut QViewTab,
        caller_silo: u64,
        nodes: Vec<QKitNode>,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if caller_silo != tab.silo_id {
            self.stats.updates_denied += 1;
            audit.log_law_violation(6u8, caller_silo, tick);
            crate::serial_println!(
                "[BROWSER] Silo {} denied widget inject into Silo {} tab (Law 6)", caller_silo, tab.silo_id
            );
            return false;
        }
        self.stats.updates_allowed += 1;
        tab.apply_widget_update(nodes);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  BrowserNavBridge: allowed={} denied={}",
            self.stats.updates_allowed, self.stats.updates_denied
        );
    }
}
