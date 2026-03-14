//! # Q-Credits Wallet Budget Bridge (Phase 181)
//!
//! ## Architecture Guardian: The Gap
//! `q_credits_wallet.rs` implements:
//! - `SpendingLimit { kind, max_per_window, window_ticks, spent_this_window, window_start_tick }`
//! - `SpendingLimit::check_and_update(amount, tick)` → bool
//! - `TxKind` — Cpu, Memory, Network, Storage, etc.
//!
//! **Missing link**: Credit spending limits were defined but never enforced
//! in the kernel hot path. A Silo could overspend without any hard gate.
//!
//! This module provides `QCreditsBudgetBridge`:
//! `charge_for_resource()` — hard enforcement of per-Silo spending limits.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::q_credits_wallet::{SpendingLimit, TxKind};

#[derive(Debug, Default, Clone)]
pub struct CreditBridgeStats {
    pub charges_allowed: u64,
    pub charges_denied:  u64,
    pub total_credits:   u64,
}

pub struct QCreditsBudgetBridge {
    limits:    BTreeMap<u64, SpendingLimit>,
    pub stats: CreditBridgeStats,
}

impl QCreditsBudgetBridge {
    pub fn new() -> Self {
        QCreditsBudgetBridge { limits: BTreeMap::new(), stats: CreditBridgeStats::default() }
    }

    /// Set per-Silo spending limit.
    pub fn set_silo_limit(
        &mut self, silo_id: u64, kind: TxKind, max_per_window: u64, window_ticks: u64,
    ) {
        self.limits.insert(silo_id, SpendingLimit {
            kind,
            max_per_window,
            window_ticks,
            spent_this_window: 0,
            window_start_tick: 0,
        });
    }

    /// Enforce spending limit for a resource charge. Returns false = deny.
    pub fn charge_for_resource(
        &mut self, silo_id: u64, amount: u64, tick: u64,
    ) -> bool {
        let limit = self.limits.entry(silo_id).or_insert(SpendingLimit {
            kind: TxKind::SpentFiberOffload,
            max_per_window: 1_000_000,
            window_ticks: 1_000,
            spent_this_window: 0,
            window_start_tick: tick,
        });
        if limit.check_and_update(amount, tick) {
            self.stats.charges_allowed += 1;
            self.stats.total_credits += amount;
            true
        } else {
            self.stats.charges_denied += 1;
            crate::serial_println!("[CREDITS] Silo {} budget exceeded: {} credits denied", silo_id, amount);
            false
        }
    }

    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.limits.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CreditsBridge: allowed={} denied={} total={}",
            self.stats.charges_allowed, self.stats.charges_denied, self.stats.total_credits
        );
    }
}
