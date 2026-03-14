//! # Q-Admin Escalation Audit Bridge (Phase 179)
//!
//! ## Architecture Guardian: The Gap
//! `q_admin.rs` implements `QAdmin` (privilege escalation manager):
//! - `request(silo_id, cap: EscalatedCap, reason, duration_secs: Option<u64>, now)` → Result<u64>
//! - `approve(token_id, auth: AuthMethod, now)` → Result<(), &str>
//!
//! **Missing link**: Privilege escalation requests were created without
//! audit logging. Escalation to Admin/Root cap access are the highest-risk
//! events in the system and must be cryptographically chained.
//!
//! This module provides `QAdminEscalationAuditBridge`:
//! 1. `request_with_audit()` — log every escalation request
//! 2. `approve_with_audit()` — log every approval

extern crate alloc;

use crate::q_admin::{QAdmin, AuthMethod, EscalatedCap};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct AdminBridgeStats {
    pub requests:  u64,
    pub approvals: u64,
    pub denials:   u64,
}

pub struct QAdminEscalationAuditBridge {
    pub admin:  QAdmin,
    pub stats:  AdminBridgeStats,
}

impl QAdminEscalationAuditBridge {
    pub fn new() -> Self {
        QAdminEscalationAuditBridge { admin: QAdmin::new(), stats: AdminBridgeStats::default() }
    }

    /// Request privilege escalation, logging the event to QAuditKernel.
    pub fn request_with_audit(
        &mut self,
        silo_id: u64,
        cap: EscalatedCap,
        reason: &str,
        duration_secs: Option<u64>,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> Option<u64> {
        self.stats.requests += 1;
        match self.admin.request(silo_id, cap, reason, duration_secs, tick) {
            Ok(token_id) => {
                audit.log_law_violation(1u8, silo_id, tick);
                crate::serial_println!("[Q-ADMIN] Silo {} escalation requested → token={}", silo_id, token_id);
                Some(token_id)
            }
            Err(e) => {
                crate::serial_println!("[Q-ADMIN] Silo {} escalation request failed: {}", silo_id, e);
                None
            }
        }
    }

    /// Approve a privilege escalation, logging the event to QAuditKernel.
    pub fn approve_with_audit(
        &mut self,
        token_id: u64,
        auth: AuthMethod,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        match self.admin.approve(token_id, auth, tick) {
            Ok(()) => {
                self.stats.approvals += 1;
                audit.log_law_violation(1u8, token_id, tick);
                true
            }
            Err(e) => {
                self.stats.denials += 1;
                crate::serial_println!("[Q-ADMIN] Escalation {} approval failed: {}", token_id, e);
                false
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QAdminBridge: requests={} approved={} denied={}",
            self.stats.requests, self.stats.approvals, self.stats.denials
        );
    }
}
