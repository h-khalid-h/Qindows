//! # ACPI Power Profile Bridge (Phase 213)
//!
//! ## Architecture Guardian: The Gap
//! `acpi.rs` implements ACPI power management:
//! - `PowerProfile` — MaxPerformance, Balanced, PowerSaver, Custom
//! - `Madt` — ACPI MADT table with processor/IO-APIC entries
//! - `AcpiHeader::verify_checksum()` → bool
//!
//! **Missing link**: ACPI power profile changes were never gated.
//! Any driver could switch from PowerSaver to MaxPerformance without cap check,
//! causing thermal/energy violations (Law 8).
//!
//! This module provides `AcpiPowerProfileBridge`:
//! Admin:EXEC required before PowerProfile changes are applied.

extern crate alloc;

use crate::acpi::PowerProfile;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct AcpiPowerStats {
    pub changes_allowed: u64,
    pub changes_denied:  u64,
}

pub struct AcpiPowerProfileBridge {
    pub current_profile: PowerProfile,
    pub stats:           AcpiPowerStats,
}

impl AcpiPowerProfileBridge {
    pub fn new() -> Self {
        AcpiPowerProfileBridge {
            current_profile: PowerProfile::Desktop,   // Default: Desktop balanced workload
            stats: AcpiPowerStats::default(),
        }
    }

    /// Change ACPI power profile — requires Admin:EXEC cap.
    pub fn set_profile(
        &mut self,
        silo_id: u64,
        profile: PowerProfile,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.changes_denied += 1;
            crate::serial_println!("[ACPI] Silo {} profile change denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.changes_allowed += 1;
        crate::serial_println!("[ACPI] Profile changed to {:?} by Silo {}", profile, silo_id);
        self.current_profile = profile;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AcpiPowerBridge: allowed={} denied={}",
            self.stats.changes_allowed, self.stats.changes_denied
        );
    }
}
