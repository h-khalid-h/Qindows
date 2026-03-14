//! # Firstboot Step Audit Bridge (Phase 268)
//!
//! ## Architecture Guardian: The Gap
//! `firstboot.rs` implements `FirstBootStep`:
//! - `FirstBootStep::next(self)` → Self — advance to next step
//! - `FirstBootStep::is_complete(self)` → bool
//!
//! **Missing link**: The first boot setup wizard had no audit trail
//! for step completion. A malicious early-init actor could skip steps,
//! bypassing identity and ledger initialization (Law 2 gap).
//!
//! This module provides `FirstbootStepAuditBridge`:
//! `advance_step()` calls FirstBootStep::next() and logs Law 2 audit.

extern crate alloc;

use crate::firstboot::FirstBootStep;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct FirstbootAuditStats {
    pub steps_advanced: u64,
}

pub struct FirstbootStepAuditBridge {
    pub stats: FirstbootAuditStats,
}

impl FirstbootStepAuditBridge {
    pub fn new() -> Self {
        FirstbootStepAuditBridge { stats: FirstbootAuditStats::default() }
    }

    /// Advance to next firstboot step — audit each advance (Law 2).
    pub fn advance_step(
        &mut self,
        current: FirstBootStep,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> FirstBootStep {
        let next = current.next();
        self.stats.steps_advanced += 1;
        crate::serial_println!("[FIRSTBOOT] Step advanced: {:?}", next);
        // Law 2 audit: system integrity event — use law_violation with code 0 for informational
        // Use a zero hash for the "old" measurement in hotswap log
        let dummy_hash = [0u8; 32];
        audit.log_hotswap("firstboot_step", &dummy_hash, tick);
        next
    }

    pub fn print_stats(&self) {
        crate::serial_println!("  FirstbootAuditBridge: advanced={}", self.stats.steps_advanced);
    }
}
