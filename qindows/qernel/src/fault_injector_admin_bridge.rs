//! # Fault Injector Admin Bridge (Phase 207)
//!
//! ## Architecture Guardian: The Gap
//! `fault_inject.rs` implements `FaultInjector`:
//! - `arm(fault_type, trigger, subsystem, max_fires, now)` → rule_id
//! - `FaultType` — MemFault, CapDeny, Timeout, IoError, etc.
//! - `Trigger` — Always, OnSysCall, OnTick, Random
//!
//! **Missing link**: Fault injection was accessible without Admin:EXEC cap.
//! Any Silo could arm a MemFault trigger and crash the kernel.
//!
//! This module provides `FaultInjectorAdminBridge`:
//! Admin:EXEC cap required before any fault rule can be armed.

extern crate alloc;

use crate::fault_inject::{FaultInjector, FaultType, Trigger};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct FaultInjectorCapStats {
    pub arms_allowed: u64,
    pub arms_denied:  u64,
}

pub struct FaultInjectorAdminBridge {
    pub injector: FaultInjector,
    pub stats:    FaultInjectorCapStats,
}

impl FaultInjectorAdminBridge {
    pub fn new() -> Self {
        FaultInjectorAdminBridge { injector: FaultInjector::new(), stats: FaultInjectorCapStats::default() }
    }

    /// Arm a fault rule — requires Admin:EXEC cap.
    pub fn arm(
        &mut self,
        silo_id: u64,
        fault_type: FaultType,
        trigger: Trigger,
        subsystem: &str,
        max_fires: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.arms_denied += 1;
            crate::serial_println!("[FAULT INJECT] Silo {} arm denied — no Admin:EXEC cap", silo_id);
            return None;
        }
        self.stats.arms_allowed += 1;
        crate::serial_println!(
            "[FAULT INJECT] Silo {} armed {:?}/{:?} on '{}'", silo_id, fault_type, trigger, subsystem
        );
        Some(self.injector.arm(fault_type, trigger, subsystem, max_fires, tick))
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FaultInjectorBridge: arms={}/{}",
            self.stats.arms_allowed, self.stats.arms_denied
        );
    }
}
