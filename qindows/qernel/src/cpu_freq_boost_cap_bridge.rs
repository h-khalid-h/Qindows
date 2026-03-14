#![no_std]

use crate::cpu_freq::CpuFreqScaler;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

/// Bridge for Phase 292: CPU Frequency Boost Admin Cap Bridge
/// Gates the `set_boost` function in `cpu_freq.rs` behind an `Admin:EXEC` capability.
pub struct CpuFreqBoostCapBridge<'a> {
    target: &'a mut CpuFreqScaler,
}

impl<'a> CpuFreqBoostCapBridge<'a> {
    pub fn new(target: &'a mut CpuFreqScaler) -> Self {
        Self { target }
    }

    pub fn set_boost(
        &mut self,
        silo_id: u64,
        enabled: bool,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if enabled && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            crate::serial_println!(
                "[CPU FREQ] Silo {} CPU Frequency Boost denied — Admin:EXEC required", silo_id
            );
            return false;
        }

        self.target.set_boost(enabled);
        true
    }
}
