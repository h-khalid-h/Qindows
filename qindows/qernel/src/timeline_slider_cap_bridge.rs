//! # Timeline Slider Cap Bridge (Phase 187)
//!
//! ## Architecture Guardian: The Gap
//! `timeline_slider.rs` implements:
//! - `TimelineNavigator::new()` — navigator
//! - `TimelineNavigator::get_timeline(current_oid: &[u8; 32])` → Option<&Timeline>
//! - `TimelineNavigator::build_timeline(current_oid, ghost_store, ...)` — build history
//! - `Timeline::set_preview(version: u32)` → bool (on a &mut Timeline)
//! - `Timeline::current_version()` → Option<&VersionEntry>
//!
//! **Missing link**: TimelineNavigator::get_timeline() was callable without cap check.
//! Any Silo could access the version history of any Prism object — a Law 9 violation.
//!
//! This module provides `TimelineSliderCapBridge`:
//! Prism:READ cap required before any timeline access.

extern crate alloc;

use crate::timeline_slider::TimelineNavigator;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ};

#[derive(Debug, Default, Clone)]
pub struct TimelineBridgeStats {
    pub reads_allowed: u64,
    pub reads_denied:  u64,
}

pub struct TimelineSliderCapBridge {
    pub navigator: TimelineNavigator,
    pub stats:     TimelineBridgeStats,
}

impl TimelineSliderCapBridge {
    pub fn new() -> Self {
        TimelineSliderCapBridge { navigator: TimelineNavigator::new(), stats: TimelineBridgeStats::default() }
    }

    /// Check access to an object's timeline — requires Prism:READ cap.
    pub fn check_timeline_access(
        &mut self,
        silo_id: u64,
        oid: &[u8; 32],
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_READ, 0, tick) {
            self.stats.reads_denied += 1;
            crate::serial_println!(
                "[TIMELINE] Silo {} access denied — no Prism:READ cap (Law 9)", silo_id
            );
            return false;
        }
        self.stats.reads_allowed += 1;
        // Return whether timeline exists for the given OID
        self.navigator.get_timeline(oid).is_some()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TimelineBridge: reads={}/{}",
            self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
