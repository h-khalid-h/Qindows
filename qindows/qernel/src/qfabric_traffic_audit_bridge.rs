//! # QFabric Traffic Audit Bridge (Phase 219)
//!
//! ## Architecture Guardian: The Gap
//! `qfabric.rs` implements Q-Mesh fabric routing between kernel nodes.
//! Fabric is the low-level interconnect layer under Nexus.
//!
//! **Missing link**: Fabric packets had no per-Silo rate limit at the
//! fabric layer (rate limiting was only at Nexus level). A burst of
//! fabric packets from one Silo could saturate the fabric bandwidth.
//!
//! This module provides `QFabricTrafficAuditBridge`:
//! Max 256 fabric packets per Silo per batch tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_FABRIC_PKTS_PER_SILO_PER_TICK: u64 = 256;

#[derive(Debug, Default, Clone)]
pub struct QFabricAuditStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct QFabricTrafficAuditBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    QFabricAuditStats,
}

impl QFabricTrafficAuditBridge {
    pub fn new() -> Self {
        QFabricTrafficAuditBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: QFabricAuditStats::default() }
    }

    pub fn allow_packet(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_FABRIC_PKTS_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QFabricBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
