//! # Intent Pipeline Rate Bridge (Phase 210)
//!
//! ## Architecture Guardian: The Gap
//! `intent_pipeline.rs` / `intent_router.rs` routes neural intent events:
//! - Intent processing pipeline for Synapse BCI input
//! - Maps NeuralPattern → IntentCategory → Silo action
//!
//! **Missing link**: The intent pipeline had no per-Silo rate limiter.
//! A Silo could generate unlimited synthetic intent events, flooding
//! the neural input queue and blocking legitimate user BCI input.
//!
//! This module provides `IntentPipelineRateBridge`:
//! Max 8 intent events per Silo per scheduler tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_INTENTS_PER_SILO_PER_TICK: u64 = 8;

#[derive(Debug, Default, Clone)]
pub struct IntentRateStats {
    pub allowed:  u64,
    pub throttled: u64,
}

pub struct IntentPipelineRateBridge {
    tick_counts: BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:   IntentRateStats,
}

impl IntentPipelineRateBridge {
    pub fn new() -> Self {
        IntentPipelineRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: IntentRateStats::default() }
    }

    /// Check if a Silo may submit another intent event this tick.
    pub fn allow_intent(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_INTENTS_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IntentRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
