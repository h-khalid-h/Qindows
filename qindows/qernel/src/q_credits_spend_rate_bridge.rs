//! # Q-Credits Spend Rate Bridge (Phase 265)
//!
//! ## Architecture Guardian: The Gap
//! `q_credits_wallet.rs` implements per-user Q-Credits:
//! - Spend operations for compute auction, storage, bandwidth
//! - CreditLedger earn/spend tracking
//!
//! **Missing link**: Q-Credits spending had no per-tick rate limit.
//! An automated Silo could exhaust a user's entire Q-Credits balance
//! in a single tick by firing thousands of micro-spend operations.
//!
//! This module provides `QCreditsSpendRateBridge`:
//! Max 100 spend operations per Silo per tick.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_SPENDS_PER_SILO_PER_TICK: u64 = 100;

#[derive(Debug, Default, Clone)]
pub struct CreditSpendRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct QCreditsSpendRateBridge {
    tick_counts:  BTreeMap<u64, u64>,
    current_tick: u64,
    pub stats:    CreditSpendRateStats,
}

impl QCreditsSpendRateBridge {
    pub fn new() -> Self {
        QCreditsSpendRateBridge { tick_counts: BTreeMap::new(), current_tick: 0, stats: CreditSpendRateStats::default() }
    }

    pub fn allow_spend(&mut self, silo_id: u64, tick: u64) -> bool {
        if tick != self.current_tick {
            self.tick_counts.clear();
            self.current_tick = tick;
        }
        let count = self.tick_counts.entry(silo_id).or_default();
        if *count >= MAX_SPENDS_PER_SILO_PER_TICK {
            self.stats.throttled += 1;
            crate::serial_println!(
                "[Q-CREDITS] Silo {} spend rate limit reached ({}/{})", silo_id, count, MAX_SPENDS_PER_SILO_PER_TICK
            );
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CreditSpendRateBridge: allowed={} throttled={}",
            self.stats.allowed, self.stats.throttled
        );
    }
}
