//! # QShell Admin Pipeline Cap Bridge (Phase 226)  
//!
//! ## Architecture Guardian: The Gap
//! `qshell.rs` implements the Q-Shell kernel interface:
//! - `Pipeline { stages, state: PipelineState, admin_escalation }`
//! - `AdminEscalation::is_valid(tick)` → bool
//! - `StageCap` — capability required for each pipeline stage
//!
//! **Missing link**: Q-Shell pipeline `AdminEscalation` tokens were
//! validated on creation but never re-checked before executing each
//! pipeline stage. An escalation that expired mid-pipeline continued.
//!
//! This module provides `QShellAdminPipelineCapBridge`:
//! Re-validates AdminEscalation on each stage before execution.

extern crate alloc;

use crate::qshell::{Pipeline, AdminEscalation};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QShellPipelineStats {
    pub stages_ok:      u64,
    pub stages_revoked: u64,
}

pub struct QShellAdminPipelineCapBridge {
    pub stats: QShellPipelineStats,
}

impl QShellAdminPipelineCapBridge {
    pub fn new() -> Self {
        QShellAdminPipelineCapBridge { stats: QShellPipelineStats::default() }
    }

    /// Validate that an AdminEscalation is still valid before running a pipeline stage.
    pub fn validate_stage(
        &mut self,
        escalation: &AdminEscalation,
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if !escalation.is_valid(tick) {
            self.stats.stages_revoked += 1;
            audit.log_law_violation(1u8, silo_id, tick);
            crate::serial_println!(
                "[QSHELL] Silo {} AdminEscalation expired mid-pipeline — stage REVOKED (Law 1)", silo_id
            );
            return false;
        }
        self.stats.stages_ok += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QShellPipelineBridge: ok={} revoked={}",
            self.stats.stages_ok, self.stats.stages_revoked
        );
    }
}
