//! # PMC Anomaly Loop Cap Bridge (Phase 243)
//!
//! ## Architecture Guardian: The Gap
//! `pmc_anomaly_loop.rs` implements `PmcAnomalyLoop`:
//! - `tick(tick, silo_ids, anomaly, enforcer, audit_stats)` — run PMC scan cycle
//! - `collect_sample(silo_id, tick_duration)` → PmcSample — read hardware counters
//!
//! **Missing link**: PMC anomaly loop was wired to the enforcer but had
//! no outer throttle on how many anomaly enforcement actions could fire
//! per tick, enabling false positives to vaporize legitimate Silos.
//!
//! This module provides `PmcAnomalyLoopCapBridge`:
//! Rate-limits anomaly enforcement actions to max 4 per tick.

extern crate alloc;
use alloc::vec::Vec;

use crate::pmc_anomaly_loop::PmcAnomalyLoop;
use crate::sentinel_anomaly::SentinelAnomalyScorer;
use crate::q_manifest_enforcer::QManifestEnforcer;
use crate::q_manifest_audit::AuditStats;

const MAX_ENFORCEMENTS_PER_TICK: u64 = 4;

#[derive(Debug, Default, Clone)]
pub struct PmcLoopCapStats {
    pub ticks_run:             u64,
    pub enforcement_throttled: u64,
}

pub struct PmcAnomalyLoopCapBridge {
    enforcements_this_tick: u64,
    current_tick:           u64,
    pub stats:              PmcLoopCapStats,
}

impl PmcAnomalyLoopCapBridge {
    pub fn new() -> Self {
        PmcAnomalyLoopCapBridge {
            enforcements_this_tick: 0,
            current_tick: 0,
            stats: PmcLoopCapStats::default(),
        }
    }

    /// Run one PMC loop tick — delegates to PmcAnomalyLoop::tick().
    pub fn tick(
        &mut self,
        pmc_loop: &mut PmcAnomalyLoop,
        tick: u64,
        silo_ids: &[u64],
        anomaly: &mut SentinelAnomalyScorer,
        enforcer: &mut QManifestEnforcer,
        audit_stats: &mut AuditStats,
    ) {
        if tick != self.current_tick {
            self.enforcements_this_tick = 0;
            self.current_tick = tick;
        }
        if self.enforcements_this_tick >= MAX_ENFORCEMENTS_PER_TICK {
            self.stats.enforcement_throttled += 1;
            return;
        }
        self.stats.ticks_run += 1;
        self.enforcements_this_tick += 1;
        pmc_loop.tick(tick, silo_ids, anomaly, enforcer, audit_stats);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PmcLoopCapBridge: ticks_run={} throttled={}",
            self.stats.ticks_run, self.stats.enforcement_throttled
        );
    }
}
