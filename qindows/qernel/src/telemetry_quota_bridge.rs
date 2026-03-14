//! # Telemetry Quota Bridge (Phase 180)
//!
//! ## Architecture Guardian: The Gap
//! `telemetry.rs` implements telemetry metric recording:
//! - `Metric::new(name, category, unit, capacity)` — metric with ring buffer
//! - `Metric::record(value: f64, now: u64)` — add data point
//! - `Metric::latest()` / `Metric::aggregate_last(n)` — query metrics
//!
//! **Missing link**: Silos could emit telemetry without limit.
//! A malicious Silo could spam high-frequency telemetry writes to exhaust
//! kernel telemetry buffer capacity, causing real metrics to be evicted
//! (ring-buffer overflow drops oldest entries).
//!
//! This module provides `TelemetryQuotaBridge`:
//! 1. `record_with_quota()` — enforce per-Silo telemetry write rate limit
//! 2. `on_tick()` — reset per-tick quota counters

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::telemetry::Metric;

/// Max telemetry data points a Silo can emit per scheduler tick (~1ms).
const MAX_DP_PER_TICK: u64 = 16;

#[derive(Debug, Default, Clone)]
pub struct TelemetryQuotaStats {
    pub records_allowed:  u64,
    pub records_throttled: u64,
}

struct SiloTelemetryState {
    used_this_tick: u64,
}

pub struct TelemetryQuotaBridge {
    silos: BTreeMap<u64, SiloTelemetryState>,
    pub stats: TelemetryQuotaStats,
}

impl TelemetryQuotaBridge {
    pub fn new() -> Self {
        TelemetryQuotaBridge { silos: BTreeMap::new(), stats: TelemetryQuotaStats::default() }
    }

    /// Record a telemetry value — enforces per-Silo rate limit.
    /// Returns true if the record was accepted.
    pub fn record_with_quota(
        &mut self,
        silo_id: u64,
        metric: &mut Metric,
        value: f64,
        tick: u64,
    ) -> bool {
        let state = self.silos.entry(silo_id).or_insert(SiloTelemetryState { used_this_tick: 0 });

        if state.used_this_tick >= MAX_DP_PER_TICK {
            self.stats.records_throttled += 1;
            return false;
        }
        state.used_this_tick += 1;
        self.stats.records_allowed += 1;
        metric.record(value, tick);
        true
    }

    /// Reset per-tick quotas at scheduler tick boundary.
    pub fn on_tick_reset(&mut self) {
        for state in self.silos.values_mut() {
            state.used_this_tick = 0;
        }
    }

    /// Remove Silo on vaporize.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.silos.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TelemetryQuotaBridge: allowed={} throttled={}",
            self.stats.records_allowed, self.stats.records_throttled
        );
    }
}
