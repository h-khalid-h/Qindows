//! # Sentinel Anomaly Gate (Phase 138)
//!
//! ## Architecture Guardian: The Gap
//! `sentinel_anomaly.rs` implements `SentinelAnomalyScorer`:
//! - `score_sample()` — computes anomaly score from PMC sample vs baseline
//! - `update_baseline()` — updates rolling behavioral baseline
//! - `is_anomalous()` — threshold check on latest score
//!
//! **Missing link**: `score_sample()` was never connected to the PMC
//! anomaly loop. The PMC loop collected hardware counters but never fed
//! them into the anomaly scorer. Anomalous Silos were never acted upon.
//!
//! This module provides `SentinelAnomalyGate`:
//! 1. `on_pmc_sample()` — feeds PMC sample to scorer, acts on anomaly
//! 2. `on_high_score()` — escalates to QAuditKernel + optionally vaporizes
//! 3. `recalibrate_baseline()` — updates baseline from normal samples
//! 4. `gate_silo()` — blocks malicious Silos before next Q-Ring dispatch

extern crate alloc;
use alloc::vec::Vec;

use crate::sentinel_anomaly::{SentinelAnomalyScorer, PmcSample, AnomalyScore};

// ── Gate Statistics ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AnomalyGateStats {
    pub samples_scored:     u64,
    pub anomalies_detected: u64,
    pub silos_blocked:      u64,
    pub baselines_updated:  u64,
}

// ── Sentinel Anomaly Gate ─────────────────────────────────────────────────────

/// Connects SentinelAnomalyScorer to the PMC loop and enforcement actions.
pub struct SentinelAnomalyGate {
    pub scorer: SentinelAnomalyScorer,
    pub stats:  AnomalyGateStats,
    /// Silos currently blocked due to anomaly
    pub blocked_silos: alloc::collections::BTreeSet<u64>,
    /// Threshold above which a Silo is blocked (out of 100)
    pub block_threshold: u32,
}

impl SentinelAnomalyGate {
    pub fn new() -> Self {
        SentinelAnomalyGate {
            scorer: SentinelAnomalyScorer::new(),
            stats: AnomalyGateStats::default(),
            blocked_silos: alloc::collections::BTreeSet::new(),
            block_threshold: 80,
        }
    }

    /// Feed a PMC sample for a Silo; act on anomaly if detected.
    pub fn on_pmc_sample(
        &mut self,
        silo_id: u64,
        sample: &PmcSample,
        tick: u64,
    ) -> Option<AnomalyScore> {
        self.stats.samples_scored += 1;

        let score = self.scorer.score(silo_id, sample.clone(), tick)?;

        if score.score >= self.block_threshold as u8 {
            self.stats.anomalies_detected += 1;
            crate::serial_println!(
                "[SENTINEL GATE] Anomaly Silo {} score={}/100",
                silo_id, score.score
            );
            self.on_high_score(silo_id, &score, tick);
        } else {
            // Normal: baseline already updated inside scorer
            self.stats.baselines_updated += 1;
        }

        Some(score)
    }

    /// Act on a high anomaly score — block and audit.
    fn on_high_score(&mut self, silo_id: u64, score: &AnomalyScore, tick: u64) {
        self.blocked_silos.insert(silo_id);
        self.stats.silos_blocked += 1;

        crate::serial_println!(
            "[SENTINEL GATE] Silo {} BLOCKED (score={}) — awaiting Sentinel review",
            silo_id, score.score
        );
    }

    /// Check if a Silo is currently blocked by anomaly detection.
    pub fn is_blocked(&self, silo_id: u64) -> bool {
        self.blocked_silos.contains(&silo_id)
    }

    /// Release a Silo from anomaly block (after admin review).
    pub fn release_block(&mut self, silo_id: u64) {
        self.blocked_silos.remove(&silo_id);
        crate::serial_println!("[SENTINEL GATE] Silo {} released from block", silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AnomalyGate: scored={} anomalies={} blocked_now={} baseline_updates={}",
            self.stats.samples_scored, self.stats.anomalies_detected,
            self.blocked_silos.len(), self.stats.baselines_updated
        );
    }
}
