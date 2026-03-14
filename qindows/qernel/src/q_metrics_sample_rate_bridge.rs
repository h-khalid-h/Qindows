//! # Q-Metrics Sample Rate Bridge (Phase 262)
//!
//! ## Architecture Guardian: The Gap
//! `q_metrics.rs` implements `MetricSample`:
//! - `MetricKind` — CpuUsage, MemPressure, IpcLatency, QRingDepth, ...
//! - `MetricSample { kind, silo_id, value, tick }`
//! - `MetricAggregate::update(value, tick)`
//!
//! **Missing link**: Metric sample submission was unthrottled. A Silo
//! could submit thousands of samples per tick, overflowing the ring
//! buffer and evicting older performance data from other Silos.
//!
//! This module provides `QMetricsSampleRateBridge`:
//! Max 32 metric samples per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_SAMPLES_PER_SILO_PER_TICK: u64 = 32;

#[derive(Debug, Default, Clone)]
pub struct MetricSampleRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct QMetricsSampleRateBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    MetricSampleRateStats,
}

impl QMetricsSampleRateBridge {
    pub fn new() -> Self {
        QMetricsSampleRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: MetricSampleRateStats::default() }
    }

    pub fn allow_sample(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_SAMPLES_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  MetricSampleRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
