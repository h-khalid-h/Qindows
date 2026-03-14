//! # QView Widget Cap Bridge (Phase 231)
//!
//! ## Architecture Guardian: The Gap
//! `q_view.rs` implements `QKitTree`:
//! - `add_widget(widget: WidgetNode)` — add UI widget to window tree
//! - `apply_delta(changed, removed)` — batch update widget tree
//! - `WidgetKind` — Button, Input, List, Canvas, ...
//!
//! **Missing link**: One Silo could inject WidgetNodes into another
//! Silo's QKitTree, overlaying UI (clickjacking / UI spoofing).
//!
//! This module provides `QViewWidgetCapBridge`:
//! Silo may only modify its own QKitTree (enforced by silo_id match).

extern crate alloc;
use alloc::vec::Vec;

use crate::q_view::{QKitTree, WidgetNode};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QViewCapStats {
    pub adds_allowed: u64,
    pub adds_denied:  u64,
}

pub struct QViewWidgetCapBridge {
    pub stats: QViewCapStats,
}

impl QViewWidgetCapBridge {
    pub fn new() -> Self {
        QViewWidgetCapBridge { stats: QViewCapStats::default() }
    }

    /// Add widget — Silo may only write to its own tree.
    pub fn add_widget(
        &mut self,
        tree: &mut QKitTree,
        tree_owner_silo: u64,
        caller_silo: u64,
        widget: WidgetNode,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if caller_silo != tree_owner_silo {
            self.stats.adds_denied += 1;
            audit.log_law_violation(6u8, caller_silo, tick); // Law 6: Silo sandbox
            crate::serial_println!(
                "[QVIEW] Silo {} denied writing to Silo {} QKitTree (UI spoofing attempt, Law 6)",
                caller_silo, tree_owner_silo
            );
            return false;
        }
        self.stats.adds_allowed += 1;
        tree.add_widget(widget);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QViewCapBridge: allowed={} denied={}", self.stats.adds_allowed, self.stats.adds_denied
        );
    }
}
