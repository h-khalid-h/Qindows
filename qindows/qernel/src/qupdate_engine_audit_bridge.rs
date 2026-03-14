//! # QUpdate Engine Audit Bridge (Phase 211)
//!
//! ## Architecture Guardian: The Gap
//! `qupdate.rs` implements `QUpdateEngine`:
//! - Package staging, queuing, and version history
//! - `UpdateTarget { Kernel, Driver, Application, Firmware }`
//!
//! **Missing link**: `QUpdateEngine` was already wrapped by `update_pipeline.rs`
//! but the engine itself had no audit log when it staged kernel/firmware updates.
//!
//! This module provides `QUpdateEngineAuditBridge`:
//! Audits all Kernel and Firmware update staging to QAuditKernel (Law 2).

extern crate alloc;

use crate::qupdate::{QUpdateEngine, UpdateTarget};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct QUpdateAuditStats {
    pub staged:          u64,
    pub kernel_audited:  u64,
    pub fw_audited:      u64,
}

pub struct QUpdateEngineAuditBridge {
    pub stats: QUpdateAuditStats,
}

impl QUpdateEngineAuditBridge {
    pub fn new() -> Self {
        QUpdateEngineAuditBridge { stats: QUpdateAuditStats::default() }
    }

    /// Audit a staged update — Kernel/Firmware updates generate Law 2 audit entries.
    pub fn audit_staged_update(
        &mut self,
        target: &UpdateTarget,
        audit: &mut QAuditKernel,
        tick: u64,
    ) {
        self.stats.staged += 1;
        match target {
            UpdateTarget::Qernel => {
                self.stats.kernel_audited += 1;
                audit.log_hotswap("kernel_update", &[0u8; 32], tick);
                crate::serial_println!("[QUPDATE AUDIT] Kernel update staged → Law 2 audit");
            }
            UpdateTarget::Firmware => {
                self.stats.fw_audited += 1;
                audit.log_hotswap("firmware_update", &[0u8; 32], tick);
                crate::serial_println!("[QUPDATE AUDIT] Firmware update staged → Law 2 audit");
            }
            _ => {
                crate::serial_println!("[QUPDATE AUDIT] {:?} update staged", target);
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QUpdateAuditBridge: staged={} kernel={} fw={}",
            self.stats.staged, self.stats.kernel_audited, self.stats.fw_audited
        );
    }
}
