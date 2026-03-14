//! # Timeline Slider Version Cap Bridge (Phase 256)
//!
//! ## Architecture Guardian: The Gap
//! `timeline_slider.rs` implements `Timeline`:
//! - `set_preview(version: u32)` → bool — preview a historical version
//! - `get_version(version)` → Option<&VersionEntry>
//! - `span_ticks()` → u64 — total timeline span
//!
//! **Missing link**: `set_preview()` had no version age cap. A Silo could
//! preview versions from thousands of ticks ago, keeping stale data live
//! and preventing timeline GC from freeing old version memory.
//!
//! This module provides `TimelineSliderVersionCapBridge`:
//! Max version age of 10000 ticks — older previews denied.

extern crate alloc;

use crate::timeline_slider::Timeline;
use crate::qaudit_kernel::QAuditKernel;

const MAX_VERSION_AGE_TICKS: u64 = 10_000;

#[derive(Debug, Default, Clone)]
pub struct TimelineVersionCapStats {
    pub previews_allowed: u64,
    pub previews_denied:  u64,
}

pub struct TimelineSliderVersionCapBridge {
    pub stats: TimelineVersionCapStats,
}

impl TimelineSliderVersionCapBridge {
    pub fn new() -> Self {
        TimelineSliderVersionCapBridge { stats: TimelineVersionCapStats::default() }
    }

    pub fn set_preview(
        &mut self,
        timeline: &mut Timeline,
        version: u32,
        tick: u64,
        audit: &mut QAuditKernel,
        silo_id: u64,
    ) -> bool {
        // Check version age
        if let Some(entry) = timeline.get_version(version) {
            if tick.saturating_sub(entry.created_at) > MAX_VERSION_AGE_TICKS {
                self.stats.previews_denied += 1;
                audit.log_law_violation(4u8, silo_id, tick);
                crate::serial_println!(
                    "[TIMELINE] Silo {} preview version {} too old — Law 4 resource fairness", silo_id, version
                );
                return false;
            }
        }
        self.stats.previews_allowed += 1;
        timeline.set_preview(version)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TimelineCapBridge: allowed={} denied={}",
            self.stats.previews_allowed, self.stats.previews_denied
        );
    }
}
