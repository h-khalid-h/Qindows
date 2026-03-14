//! # Update Pipeline Rate Bridge (Phase 269)
//!
//! ## Architecture Guardian: The Gap
//! `update_pipeline.rs` implements the system update orchestrator:
//! - Update staging, verification, application (hotswap)
//! - Rollback on hash failure
//!
//! **Missing link**: Update staging had no per-session frequency cap.
//! An automated process could stage+apply updates in rapid succession,
//! creating a rapid rollback storm that destabilizes the kernel state.
//!
//! This module provides `UpdatePipelineRateBridge`:
//! Max 2 full update cycles per 1000 ticks.

extern crate alloc;

const MIN_TICKS_BETWEEN_UPDATES: u64 = 500;

#[derive(Debug, Default, Clone)]
pub struct UpdatePipelineRateStats {
    pub updates_allowed: u64,
    pub updates_denied:  u64,
}

pub struct UpdatePipelineRateBridge {
    last_update_tick: u64,
    pub stats:        UpdatePipelineRateStats,
}

impl UpdatePipelineRateBridge {
    pub fn new() -> Self {
        UpdatePipelineRateBridge { last_update_tick: 0, stats: UpdatePipelineRateStats::default() }
    }

    pub fn allow_update(&mut self, tick: u64) -> bool {
        if tick.saturating_sub(self.last_update_tick) < MIN_TICKS_BETWEEN_UPDATES {
            self.stats.updates_denied += 1;
            crate::serial_println!(
                "[UPDATE] Update denied — min {} ticks between updates (last at {})", MIN_TICKS_BETWEEN_UPDATES, self.last_update_tick
            );
            return false;
        }
        self.last_update_tick = tick;
        self.stats.updates_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UpdatePipelineRateBridge: allowed={} denied={}",
            self.stats.updates_allowed, self.stats.updates_denied
        );
    }
}
