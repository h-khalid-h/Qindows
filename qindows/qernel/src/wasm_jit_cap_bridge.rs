//! # WASM JIT Cap Bridge (Phase 174)
//!
//! ## Architecture Guardian: The Gap
//! `wasm_runtime.rs` provides:
//! - `validate_wasm_binary(bytes)` → Result<WasmModuleDesc, WasmValidationError>
//! - `WasmModuleDesc` — validated module descriptor
//! - `WasmMemoryPlan::plan(&WasmModuleDesc)` — memory layout
//!
//! **Missing link**: WASM modules were loaded without cap checks.
//! Wasm:EXEC CapType existed but was never enforced at load time.
//!
//! This module provides `WasmJitCapBridge`:
//! 1. `validate_with_cap_check()` — Wasm:EXEC required before load
//! 2. `plan_memory_with_cap_check()` — Wasm:EXEC required before memory plan

extern crate alloc;

use crate::wasm_runtime::{validate_wasm_binary, WasmModuleDesc, WasmMemoryPlan, WasmValidationError};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct WasmCapBridgeStats {
    pub loads_allowed: u64,
    pub loads_denied:  u64,
}

pub struct WasmJitCapBridge {
    pub stats: WasmCapBridgeStats,
}

impl WasmJitCapBridge {
    pub fn new() -> Self {
        WasmJitCapBridge { stats: WasmCapBridgeStats::default() }
    }

    /// Validate and load a WASM module — requires Wasm:EXEC cap.
    pub fn validate_with_cap_check(
        &mut self,
        silo_id: u64,
        bytecode: &[u8],
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Result<WasmModuleDesc, WasmValidationError> {
        if !forge.check(silo_id, CapType::Wasm, CAP_EXEC, 0, tick) {
            self.stats.loads_denied += 1;
            crate::serial_println!("[WASM CAP] Silo {} WASM load denied — no Wasm:EXEC cap", silo_id);
            return Err(WasmValidationError::InvalidMagic);
        }
        self.stats.loads_allowed += 1;
        validate_wasm_binary(bytecode)
    }

    /// Plan memory layout for a validated WASM module (Wasm:EXEC required).
    pub fn plan_memory_with_cap_check(
        &mut self,
        silo_id: u64,
        desc: &WasmModuleDesc,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<WasmMemoryPlan> {
        if !forge.check(silo_id, CapType::Wasm, CAP_EXEC, 0, tick) {
            self.stats.loads_denied += 1;
            return None;
        }
        Some(WasmMemoryPlan::plan(desc))
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WasmCapBridge: allowed={} denied={}",
            self.stats.loads_allowed, self.stats.loads_denied
        );
    }
}
