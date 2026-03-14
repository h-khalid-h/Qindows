//! # Core Dump Audit Bridge (Phase 158)
//!
//! ## Architecture Guardian: The Gap
//! `coredump.rs` `CoreDumpEngine::capture(silo_id, reason, regs, tick)` was
//! never called from fault handlers or Silo vaporize path.
//!
//! DumpReason variants: KernelPanic, PageFault, GeneralProtectionFault,
//! DoubleFault, StackOverflow, WatchdogTimeout, UserRequested, AssertionFailed,
//! OutOfMemory, InvalidOpcode.
//!
//! QAuditKernel API: log_law_violation(law, silo_id, tick),
//! log_cap_denial(silo_id, cap_type_str, tick), log_silo_vaporize(silo_id, reason, tick).
//!
//! This module provides `CoreDumpAuditBridge`:
//! 1. `on_silo_crash()` — capture + audit on crash
//! 2. `on_fault()` — capture + audit on hardware fault

extern crate alloc;

use crate::coredump::{DumpManager, DumpReason, DumpConfig, CpuRegisters};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct CoreDumpBridgeStats {
    pub crashes_captured: u64,
    pub fault_dumps:      u64,
}

pub struct CoreDumpAuditBridge {
    pub engine: DumpManager,
    pub stats:  CoreDumpBridgeStats,
}

impl CoreDumpAuditBridge {
    pub fn new() -> Self {
        CoreDumpAuditBridge {
            engine: DumpManager::new(DumpConfig::default()),
            stats:  CoreDumpBridgeStats::default(),
        }
    }

    /// Called when a Silo crashes (e.g. WatchdogTimeout or OOM).
    /// Captures dump and records audit trail (Law 8).
    pub fn on_silo_crash(
        &mut self,
        silo_id: u64,
        regs: CpuRegisters,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> u64 {
        self.stats.crashes_captured += 1;
        let dump_id = self.engine.capture(DumpReason::WatchdogTimeout, regs, Some(silo_id), 0, None, tick);

        // Law 8: all Silo termination events must be audit-logged
        audit.log_silo_vaporize(silo_id, &alloc::format!("crash: dump {}", dump_id), tick);

        crate::serial_println!(
            "[COREDUMP] Silo {} crash → dump {} (Law 8 audit)", silo_id, dump_id
        );
        dump_id
    }

    /// Called on hardware fault (PageFault, GPFault, InvalidOpcode) in a Silo.
    pub fn on_fault(
        &mut self,
        silo_id: u64,
        reason: DumpReason,
        regs: CpuRegisters,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> u64 {
        self.stats.fault_dumps += 1;
        let dump_id = self.engine.capture(reason, regs, Some(silo_id), 0, None, tick);

        audit.log_law_violation(6, silo_id, tick); // Law 6: Silo sandbox violated

        crate::serial_println!(
            "[COREDUMP] Silo {} fault → dump {}", silo_id, dump_id
        );
        dump_id
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CoreDumpBridge: crashes={} faults={}",
            self.stats.crashes_captured, self.stats.fault_dumps
        );
    }
}
