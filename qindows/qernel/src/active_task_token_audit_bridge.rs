//! # Active Task Token Audit Bridge (Phase 221)
//!
//! ## Architecture Guardian: The Gap
//! `active_task.rs` implements `ActiveTaskToken`:
//! - `new(silo_id, category: TaskCategory, reason, tick)` → token
//! - `is_expired(tick)` → bool
//! - `can_renew()` → bool
//! - `TaskCategory { Realtime, Interactive, Background, Maintenance, ... }`
//!
//! **Missing link**: Expired task tokens were never enforced at scheduling.
//! A Realtime token that expired was never revoked — the Silo continued
//! using its high-priority scheduling slot after authorization ended.
//!
//! This module provides `ActiveTaskTokenAuditBridge`:
//! Returns false on `tick_check()` if token is expired (Law 1).

extern crate alloc;

use crate::active_task::{ActiveTaskToken, TaskCategory};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct TaskTokenStats {
    pub checks:    u64,
    pub expired:   u64,
}

pub struct ActiveTaskTokenAuditBridge {
    pub stats: TaskTokenStats,
}

impl ActiveTaskTokenAuditBridge {
    pub fn new() -> Self {
        ActiveTaskTokenAuditBridge { stats: TaskTokenStats::default() }
    }

    /// Verify a task token is still valid. Returns false (and audits) if expired.
    pub fn tick_check(
        &mut self,
        token: &ActiveTaskToken,
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        self.stats.checks += 1;
        if token.is_expired(tick) {
            self.stats.expired += 1;
            audit.log_law_violation(1u8, silo_id, tick);
            crate::serial_println!(
                "[TASK TOKEN] Silo {} token expired — Law 1 audit, should reschedule", silo_id
            );
            return false;
        }
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TaskTokenBridge: checks={} expired={}",
            self.stats.checks, self.stats.expired
        );
    }
}
