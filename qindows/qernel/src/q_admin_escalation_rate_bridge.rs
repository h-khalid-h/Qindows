//! # Q-Admin Escalation Rate Bridge (Phase 277)
//!
//! ## Architecture Guardian: The Gap
//! `q_admin.rs` implements the Q-Admin system:
//! - `EscalationToken::is_valid(now: u64)` → bool
//! - `EscalatedCap` — Admin capability after escalation
//! - `EscalationState` — Pending, Active, Expired, Revoked
//!
//! **Missing link**: Escalation token requests had no rate limit.
//! A Silo could spam escalation requests, filling the admin queue
//! and blocking legitimate admin operations (Law 4 DoS).
//!
//! This module provides `QAdminEscalationRateBridge`:
//! Max 4 escalation requests per Silo per 100 ticks.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_ESCALATIONS_PER_100_TICKS: u64 = 4;
const WINDOW_TICKS: u64 = 100;

#[derive(Debug, Default, Clone)]
pub struct EscalationRateStats {
    pub allowed:   u64,
    pub throttled: u64,
}

pub struct QAdminEscalationRateBridge {
    silo_escalations: BTreeMap<u64, (u64, u64)>, // silo_id → (window_start, count)
    pub stats:        EscalationRateStats,
}

impl QAdminEscalationRateBridge {
    pub fn new() -> Self {
        QAdminEscalationRateBridge { silo_escalations: BTreeMap::new(), stats: EscalationRateStats::default() }
    }

    pub fn allow_escalation(&mut self, silo_id: u64, tick: u64) -> bool {
        let entry = self.silo_escalations.entry(silo_id).or_insert((tick, 0));
        if tick.saturating_sub(entry.0) >= WINDOW_TICKS {
            *entry = (tick, 0); // reset window
        }
        if entry.1 >= MAX_ESCALATIONS_PER_100_TICKS {
            self.stats.throttled += 1;
            crate::serial_println!(
                "[Q-ADMIN] Silo {} escalation rate limit ({}/{})", silo_id, entry.1, MAX_ESCALATIONS_PER_100_TICKS
            );
            return false;
        }
        entry.1 += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EscalationRateBridge: allowed={} throttled={}", self.stats.allowed, self.stats.throttled
        );
    }
}
