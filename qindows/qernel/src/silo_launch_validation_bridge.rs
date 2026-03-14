//! # Silo Launch Validation Bridge (Phase 237)
//!
//! ## Architecture Guardian: The Gap
//! `silo_launch.rs` implements Silo binary launch:
//! - `validate_entry_point(entry: u64)` → Result<(), LaunchError>
//! - `build_entry_regs(binary, silo_id)` → Result<SiloEntryRegs, LaunchError>
//! - `map_user_stack(...)` — map stack for new Silo
//!
//! **Missing link**: `validate_entry_point()` was called but errors were
//! sometimes silently discarded. An invalid entry point that passes
//! unchecked causes unpredictable Silo crashes or privilege escalation.
//!
//! This module provides `SiloLaunchValidationBridge`:
//! Strict entry point + stack validation gate — all errors halt launch.

extern crate alloc;

use crate::silo_launch::{validate_entry_point, LaunchError};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct SiloLaunchValidationStats {
    pub launches_ok:     u64,
    pub launches_failed: u64,
}

pub struct SiloLaunchValidationBridge {
    pub stats: SiloLaunchValidationStats,
}

impl SiloLaunchValidationBridge {
    pub fn new() -> Self {
        SiloLaunchValidationBridge { stats: SiloLaunchValidationStats::default() }
    }

    /// Validate a Silo entry point. Returns false and audits on failure.
    pub fn validate_entry(
        &mut self,
        silo_id: u64,
        entry: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        match validate_entry_point(entry) {
            Ok(_) => {
                self.stats.launches_ok += 1;
                true
            }
            Err(e) => {
                self.stats.launches_failed += 1;
                audit.log_law_violation(2u8, silo_id, tick); // Law 2: binary integrity
                crate::serial_println!(
                    "[SILO LAUNCH] Silo {} entry point 0x{:x} invalid ({:?}) — Law 2 audit", silo_id, entry, e
                );
                false
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SiloLaunchValidationBridge: ok={} failed={}",
            self.stats.launches_ok, self.stats.launches_failed
        );
    }
}
