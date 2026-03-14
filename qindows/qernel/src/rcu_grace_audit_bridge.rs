//! # RCU Grace Period Audit Bridge (Phase 217)
//!
//! ## Architecture Guardian: The Gap
//! `rcu.rs` implements Read-Copy-Update:
//! - `RcuManager { version, grace_state, cpu_states }`
//! - `read_lock(cpu_id)` / `read_unlock(cpu_id)`
//! - `publish(object_id, now)` → version
//! - `advance_grace_period()` — advance to next grace period
//!
//! **Missing link**: Grace periods were advanced without rate control.
//! Rapid `advance_grace_period()` calls could exhaust deferred callback
//! memory allocations. No audit trail for grace period advancement.
//!
//! This module provides `RcuGracePeriodAuditBridge`:
//! Tracks grace period frequency and logs excessive rates to audit.

extern crate alloc;

use crate::rcu::RcuManager;
use crate::qaudit_kernel::QAuditKernel;

const MAX_GRACE_ADVANCES_PER_TICK: u64 = 16;

#[derive(Debug, Default, Clone)]
pub struct RcuGraceStats {
    pub advances:    u64,
    pub rate_limited: u64,
}

pub struct RcuGracePeriodAuditBridge {
    pub rcu:              RcuManager,
    advances_this_tick:  u64,
    current_tick:        u64,
    pub stats:           RcuGraceStats,
}

impl RcuGracePeriodAuditBridge {
    pub fn new() -> Self {
        RcuGracePeriodAuditBridge { rcu: RcuManager::new(), advances_this_tick: 0, current_tick: 0, stats: RcuGraceStats::default() }
    }

    /// Advance grace period — rate-limited to MAX_GRACE_ADVANCES_PER_TICK.
    pub fn advance_grace_period(&mut self, tick: u64, audit: &mut QAuditKernel) {
        if tick != self.current_tick {
            self.advances_this_tick = 0;
            self.current_tick = tick;
        }
        if self.advances_this_tick >= MAX_GRACE_ADVANCES_PER_TICK {
            self.stats.rate_limited += 1;
            audit.log_law_violation(4u8, 0, tick);
            return;
        }
        self.advances_this_tick += 1;
        self.stats.advances += 1;
        self.rcu.advance_grace_period();
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  RcuGraceBridge: advances={} rate_limited={}",
            self.stats.advances, self.stats.rate_limited
        );
    }
}
