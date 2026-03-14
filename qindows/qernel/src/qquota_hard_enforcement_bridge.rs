//! # QQuota Hard Enforcement Bridge (Phase 223)
//!
//! ## Architecture Guardian: The Gap
//! `qquota.rs` implements `SiloQuota`:
//! - `QuotaEntry::would_exceed(amount)` → bool
//! - `QuotaEntry::over_soft()` → bool
//! - `SiloQuota` — per-Silo quota tracking
//! - `QuotaResult` — Ok, SoftWarning, HardDenied
//!
//! **Missing link**: QuotaResult::HardDenied was returned but not actually
//! enforced — the caller could ignore it and proceed anyway. No audit trail
//! when hard limits were breached.
//!
//! This module provides `QQuotaHardEnforcementBridge`:
//! Logs Law 4 audit on hard quota denial and returns unambiguous bool.

extern crate alloc;

use crate::qquota::QuotaResult;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QQuotaEnforcementStats {
    pub ok:          u64,
    pub soft:        u64,
    pub hard_denied: u64,
}

pub struct QQuotaHardEnforcementBridge {
    pub stats: QQuotaEnforcementStats,
}

impl QQuotaHardEnforcementBridge {
    pub fn new() -> Self {
        QQuotaHardEnforcementBridge { stats: QQuotaEnforcementStats::default() }
    }

    /// Convert QuotaResult to a hard bool gate — false means the operation MUST be blocked.
    pub fn enforce(
        &mut self,
        result: QuotaResult,
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        match result {
            QuotaResult::Allowed | QuotaResult::NoQuota => { self.stats.ok += 1; true }
            QuotaResult::SoftWarning => {
                self.stats.soft += 1;
                crate::serial_println!("[QQUOTA] Silo {} soft limit exceeded — warning", silo_id);
                true  // allow but warn
            }
            QuotaResult::HardDenied => {
                self.stats.hard_denied += 1;
                audit.log_law_violation(4u8, silo_id, tick);
                crate::serial_println!(
                    "[QQUOTA] Silo {} HARD DENIED — Law 4 audit", silo_id
                );
                false  // must block
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QQuotaBridge: ok={} soft={} hard_denied={}",
            self.stats.ok, self.stats.soft, self.stats.hard_denied
        );
    }
}
