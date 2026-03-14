//! # QRing Dispatch Rate Bridge (Phase 252)
//!
//! ## Architecture Guardian: The Gap
//! `qring_dispatch.rs` implements the real Q-Ring dispatch:
//! - `dispatch(silo_id, entry: &SqEntry, opcode: SqOpcode, tick)` → RealDispatchResult
//! - `SqEntry` and `SqOpcode` are re-exported from `qring_async`
//!
//! **Missing link**: `dispatch()` had no per-Silo rate limit. A Silo
//! could submit unlimited SqEntries in rapid succession, exhausting
//! the completion queue and starving other Silos of I/O.
//!
//! This module provides `QRingDispatchRateBridge`:
//! Max 128 dispatches per Silo per tick (Law 4).

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::qring_async::{SqEntry, SqOpcode, CompStatus};
use crate::qring_dispatch::{RealDispatchResult, dispatch};
use crate::qaudit_kernel::QAuditKernel;

const MAX_DISPATCHES_PER_SILO_PER_TICK: u64 = 128;

#[derive(Debug, Default, Clone)]
pub struct QRingDispatchRateStats {
    pub dispatched:  u64,
    pub throttled:   u64,
}

pub struct QRingDispatchRateBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    QRingDispatchRateStats,
}

impl QRingDispatchRateBridge {
    pub fn new() -> Self {
        QRingDispatchRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: QRingDispatchRateStats::default() }
    }

    pub fn dispatch(
        &mut self,
        silo_id: u64,
        entry: &SqEntry,
        opcode: SqOpcode,
        tick: u64,
        audit: &mut QAuditKernel,
    ) -> Option<RealDispatchResult> {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_DISPATCHES_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            audit.log_law_violation(4u8, silo_id, tick); // Law 4: resource fairness
            return None;
        }
        *count += 1;
        self.stats.dispatched += 1;
        Some(dispatch(silo_id, entry, opcode, tick))
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QRingDispatchRateBridge: dispatched={} throttled={}",
            self.stats.dispatched, self.stats.throttled
        );
    }
}
