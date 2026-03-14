//! # QTraffic Flow Account Cap Bridge (Phase 279)
//!
//! ## Architecture Guardian: The Gap
//! `qtraffic.rs` implements `SiloTrafficAccount`:
//! - `SiloTrafficAccount::new(silo_id)` — per-Silo flow tracking
//! - `SiloTrafficAccount::record_flow(ev: FlowEvent)` — record traffic
//! - `FlowEvent { direction, proto, bytes, ... }`
//!
//! **Missing link**: `record_flow()` was called per-packet without rate
//! checking. A Silo generating millions of micro-flows per tick would
//! fill the traffic account event buffer, evicting older flow records
//! and preventing Law 7 auditing of earlier traffic.
//!
//! This module provides `QTrafficFlowAccountCapBridge`:
//! Max 256 flow events recorded per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_FLOWS_PER_SILO_PER_TICK: u64 = 256;

#[derive(Debug, Default, Clone)]
pub struct TrafficFlowCapStats {
    pub recorded:  u64,
    pub throttled: u64,
}

pub struct QTrafficFlowAccountCapBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    TrafficFlowCapStats,
}

impl QTrafficFlowAccountCapBridge {
    pub fn new() -> Self {
        QTrafficFlowAccountCapBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: TrafficFlowCapStats::default() }
    }

    pub fn allow_record_flow(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_FLOWS_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.recorded += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TrafficFlowCapBridge: recorded={} throttled={}",
            self.stats.recorded, self.stats.throttled
        );
    }
}
