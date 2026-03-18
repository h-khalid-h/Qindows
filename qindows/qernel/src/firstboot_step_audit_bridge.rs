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
        // Derive a real measurement hash for Law 2 audit integrity.
        // Mix step discriminant, tick, and a boot constant to get a unique per-step measurement.
        let step_idx = next as u64;
        let mut step_hash = [0u8; 32];
        let tick_bytes = tick.to_le_bytes();
        let step_bytes = step_idx.to_le_bytes();
        // XOR-fold to create a simple deterministic measurement (no crypto dep needed here)
        for i in 0..8 { step_hash[i] = tick_bytes[i] ^ step_bytes[i % 8]; }
        for i in 8..16 { step_hash[i] = tick_bytes[i - 8].wrapping_add(step_bytes[i % 8]); }
        for i in 16..24 { step_hash[i] = 0xAB ^ tick_bytes[i - 16] ^ step_bytes[0]; }
        for i in 24..32 { step_hash[i] = 0xCD ^ step_bytes[i % 8]; }
        audit.log_hotswap("firstboot_step", &step_hash, tick);
        next
    }

    pub fn print_stats(&self) {
        crate::serial_println!("  FirstbootAuditBridge: advanced={}", self.stats.steps_advanced);
    }
}
