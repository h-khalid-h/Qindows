//! # Update Pipeline Audit Bridge (Phase 189)
//!
//! ## Architecture Guardian: The Gap
//! `update_pipeline.rs` implements staged kernel/app updates:
//! - `UpdatePipeline::stage(package: UpdatePackage, package_bytes, tick)` → update_id
//! - `UpdatePipeline::apply_next(package_bytes, tick, enforcer, audit_stats)` → Option<UpdateResult>
//! - `UpdatePipeline::rollback(update_id, reason)` → bool
//!
//! **Missing link**: Any code path could call apply_next without an Admin:EXEC
//! CapToken check. A compromised Silo could trigger unauthorized system updates.
//!
//! This module provides `UpdatePipelineAuditBridge`:
//! Admin:EXEC gate is enforced before any delegation to UpdatePipeline.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct UpdateAuditStats {
    pub gates_allowed: u64,
    pub gates_denied:  u64,
}

pub struct UpdatePipelineAuditBridge {
    pub stats: UpdateAuditStats,
}

impl UpdatePipelineAuditBridge {
    pub fn new() -> Self {
        UpdatePipelineAuditBridge { stats: UpdateAuditStats::default() }
    }

    /// Check if a Silo is authorized to trigger updates.
    /// Returns true (authorized) or false (denied). Call this BEFORE invoking UpdatePipeline.
    pub fn authorize_update(
        &mut self,
        silo_id: u64,
        forge: &mut CapTokenForge,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.gates_denied += 1;
            crate::serial_println!("[UPDATE AUDIT] Silo {} update DENIED — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.gates_allowed += 1;
        // Log the authorization event via audit chain
        audit.log_hotswap("update_pipeline", &[0u8; 32], tick);
        crate::serial_println!("[UPDATE AUDIT] Silo {} update authorized (Law 2 + Law 8 audit)", silo_id);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UpdateAuditBridge: allowed={} denied={}",
            self.stats.gates_allowed, self.stats.gates_denied
        );
    }
}
