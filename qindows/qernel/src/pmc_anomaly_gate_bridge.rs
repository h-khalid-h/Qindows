//! # PMC Anomaly Gate Bridge (Phase 200)
//!
//! ## Architecture Guardian: The Gap
//! `pmc.rs` implements `PmcMonitor` with `PmcSample`, `AnomalyThresholds`, `PmcAnomaly`:
//! `sentinel_anomaly.rs` implements `SentinelAnomalyGate`:
//! - `on_pmc_sample(silo_id, sample: &PmcSample, tick)` → bool (block this Silo?)
//! - `is_blocked(silo_id)` → bool
//! - `release_block(silo_id)`
//!
//! **Missing link**: PMC anomaly detection and the SentinelAnomalyGate were
//! implemented independently with no integration path. A Silo exhibiting
//! Rowhammer, Spectre, or cache side-channel patterns was detected but
//! never actually blocked by the scheduler.
//!
//! This module provides `PmcAnomalyGateBridge`:
//! Wires PmcSample → SentinelAnomalyGate → block/unblock Silo scheduling.

extern crate alloc;

use crate::sentinel_anomaly_gate::SentinelAnomalyGate;
use crate::sentinel_anomaly::PmcSample;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct PmcGateBridgeStats {
    pub samples:   u64,
    pub blocked:   u64,
    pub unblocked: u64,
}

pub struct PmcAnomalyGateBridge {
    pub gate:  SentinelAnomalyGate,
    pub stats: PmcGateBridgeStats,
}

impl PmcAnomalyGateBridge {
    pub fn new() -> Self {
        PmcAnomalyGateBridge { gate: SentinelAnomalyGate::new(), stats: PmcGateBridgeStats::default() }
    }

    /// Process a PMC sample — blocks Silo if anomaly detected.
    /// Returns false if Silo should be removed from scheduler queue.
    pub fn observe_and_gate(
        &mut self,
        silo_id: u64,
        sample: &PmcSample,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        self.stats.samples += 1;
        // Some(AnomalyScore) = anomaly detected; None = clean
        match self.gate.on_pmc_sample(silo_id, sample, tick) {
            Some(_score) => {
                self.stats.blocked += 1;
                audit.log_law_violation(6u8, silo_id, tick);
                crate::serial_println!(
                    "[PMC GATE] Silo {} BLOCKED — PMC anomaly (Law 6)", silo_id
                );
                false // block
            }
            None => true, // no anomaly — allow
        }
    }

    pub fn is_blocked(&self, silo_id: u64) -> bool {
        self.gate.is_blocked(silo_id)
    }

    /// Release a Silo that has been cleared by manual admin review.
    pub fn release_block(&mut self, silo_id: u64) {
        self.stats.unblocked += 1;
        self.gate.release_block(silo_id);
        crate::serial_println!("[PMC GATE] Silo {} released from anomaly block", silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PmcGateBridge: samples={} blocked={} unblocked={}",
            self.stats.samples, self.stats.blocked, self.stats.unblocked
        );
    }
}
