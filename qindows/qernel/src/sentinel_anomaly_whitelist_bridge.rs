//! # Sentinel Anomaly Whitelist Bridge (Phase 274)
//!
//! ## Architecture Guardian: The Gap
//! `sentinel_anomaly.rs` implements `SentinelAnomalyScorer`:
//! - `score(silo_id, sample: PmcSample, tick)` → Option<ScoreResult>
//! - PMC-based anomaly detection with threshold scoring
//!
//! **Missing link**: The anomaly scorer applied uniform scoring to all
//! Silos — including known legitimate high-workload Silos (system
//! services like Prism and Aether). This caused false-positive flagging
//! of legitimate background I/O workloads.
//!
//! This module provides `SentinelAnomalyWhitelistBridge`:
//! System Silo whitelist — skip anomaly scoring for trusted system IDs.

extern crate alloc;
use alloc::vec::Vec;

#[derive(Debug, Default, Clone)]
pub struct AnomalyWhitelistStats {
    pub whitelisted_skips: u64,
    pub scored:            u64,
}

pub struct SentinelAnomalyWhitelistBridge {
    whitelist: Vec<u64>,
    pub stats: AnomalyWhitelistStats,
}

impl SentinelAnomalyWhitelistBridge {
    pub fn new(system_silo_ids: Vec<u64>) -> Self {
        SentinelAnomalyWhitelistBridge { whitelist: system_silo_ids, stats: AnomalyWhitelistStats::default() }
    }

    /// Returns true if scoring should proceed (silo not whitelisted).
    pub fn should_score(&mut self, silo_id: u64) -> bool {
        if self.whitelist.contains(&silo_id) {
            self.stats.whitelisted_skips += 1;
            return false;
        }
        self.stats.scored += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AnomalyWhitelistBridge: scored={} skipped={}", self.stats.scored, self.stats.whitelisted_skips
        );
    }
}
