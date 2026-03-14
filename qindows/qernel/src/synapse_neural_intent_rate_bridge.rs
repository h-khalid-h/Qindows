//! # Synapse Neural Intent Rate Bridge (Phase 281)
//!
//! ## Architecture Guardian: The Gap
//! `synapse.rs` implements the Q-Synapse BCI:
//! - `IntentEvent { category: IntentCategory, neural_pattern, confidence }`
//! - `NeuralBinding { silo_id, pattern_hash, ... }`
//! - `ThoughtGateState` — guard for unauthorized intent routing
//!
//! **Missing link**: Neural intent events had no rate limit per binding.
//! A malicious BCI device could flood the intent pipeline with synthetic
//! intents, dominating the intent router and starving genuine intents.
//!
//! This module provides `SynapseNeuralIntentRateBridge`:
//! Max 32 intent events per neural binding per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_INTENTS_PER_BINDING_PER_TICK: u64 = 32;

#[derive(Debug, Default, Clone)]
pub struct NeuralIntentRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct SynapseNeuralIntentRateBridge {
    tick_counts:  BTreeMap<u64, u64>, // silo_id → intent count
    current_tick: u64,
    pub stats:    NeuralIntentRateStats,
}

impl SynapseNeuralIntentRateBridge {
    pub fn new() -> Self {
        SynapseNeuralIntentRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: NeuralIntentRateStats::default() }
    }

    pub fn allow_intent(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_INTENTS_PER_BINDING_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NeuralIntentRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
