//! # Boot Sequence Integrity Bridge (Phase 230)
//!
//! ## Architecture Guardian: The Gap
//! `boot_sequence.rs` implements the kernel boot orchestration.
//!
//! **Missing link**: The boot sequence had no integrity checkpoint.
//! Individual boot stages ran without verifying the previous stage
//! completed successfully and unmodified.
//!
//! This module provides `BootSequenceIntegrityBridge`:
//! Tracks boot stage completion and verifies stage ordering (Law 2).

extern crate alloc;
use alloc::vec::Vec;

use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStage {
    HardwareInit = 0,
    SecureBoot   = 1,
    MemorySetup  = 2,
    KernelLoad   = 3,
    CapsInit     = 4,
    SiloLaunch   = 5,
    Complete     = 6,
}

#[derive(Debug, Default, Clone)]
pub struct BootIntegrityStats {
    pub stages_ok:      u64,
    pub order_failures: u64,
}

pub struct BootSequenceIntegrityBridge {
    last_stage:  Option<BootStage>,
    pub stats:   BootIntegrityStats,
}

impl BootSequenceIntegrityBridge {
    pub fn new() -> Self {
        BootSequenceIntegrityBridge { last_stage: None, stats: BootIntegrityStats::default() }
    }

    /// Record that a boot stage has completed. Returns false if stage is out of order.
    pub fn stage_complete(
        &mut self,
        stage: BootStage,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        let expected = match self.last_stage {
            None => 0,
            Some(s) => s as u8 + 1,
        };
        if (stage as u8) != expected {
            self.stats.order_failures += 1;
            audit.log_law_violation(2u8, 0, tick); // Law 2: boot integrity chain
            crate::serial_println!(
                "[BOOT SEQ] Stage ordering violation: expected {}, got {} (Law 2)",
                expected, stage as u8
            );
            return false;
        }
        self.last_stage = Some(stage);
        self.stats.stages_ok += 1;
        crate::serial_println!("[BOOT SEQ] Stage {:?} completed OK", stage);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  BootSeqBridge: stages_ok={} order_failures={}",
            self.stats.stages_ok, self.stats.order_failures
        );
    }
}
