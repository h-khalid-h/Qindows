//! # WASM Sandbox Exec Bridge (Phase 188 — New)
//!
//! ## Architecture Guardian: The Gap
//! `sandbox.rs` implements `SandboxManager`:
//! - `create(module_name, module_hash:[u8;32], silo_id, capabilities:u64, limits:Option<ResourceLimits>)` → u64
//! - `load(sandbox_id)` → Result<(), &str>
//! - `run(sandbox_id)` → Result<i64, TrapReason>
//! - `kill(sandbox_id)` — terminate sandbox
//!
//! **Missing context**: The existing Phase 133 `sandbox_cap_bridge` connects
//! TrapReason to Laws. This bridge (Phase 188) adds a second enforcement layer:
//! ensuring the Wasm:EXEC cap check happens specifically before sandbox *load*
//! and *run*, which Phase 133 didn't enforce for `load()`.
//!
//! This module provides `WasmSandboxExecBridge`:
//! Wasm:EXEC gate on load + run (not just create).

extern crate alloc;

use crate::sandbox::{SandboxManager, TrapReason};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct WasmExecBridgeStats {
    pub loads_allowed: u64,
    pub loads_denied:  u64,
    pub runs_allowed:  u64,
    pub trap_count:    u64,
}

pub struct WasmSandboxExecBridge {
    pub manager: SandboxManager,
    pub stats:   WasmExecBridgeStats,
}

impl WasmSandboxExecBridge {
    pub fn new() -> Self {
        WasmSandboxExecBridge { manager: SandboxManager::new(), stats: WasmExecBridgeStats::default() }
    }

    /// Load a sandbox binary — requires Wasm:EXEC cap.
    pub fn load_with_cap_check(
        &mut self,
        silo_id: u64,
        sandbox_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Wasm, CAP_EXEC, 0, tick) {
            self.stats.loads_denied += 1;
            return false;
        }
        self.stats.loads_allowed += 1;
        self.manager.load(sandbox_id).is_ok()
    }

    /// Run a sandbox — requires Wasm:EXEC cap.
    pub fn run_with_cap_check(
        &mut self,
        silo_id: u64,
        sandbox_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<i64> {
        if !forge.check(silo_id, CapType::Wasm, CAP_EXEC, 0, tick) {
            self.stats.loads_denied += 1;
            return None;
        }
        self.stats.runs_allowed += 1;
        match self.manager.run(sandbox_id) {
            Ok(exit) => Some(exit),
            Err(trap) => {
                self.stats.trap_count += 1;
                crate::serial_println!("[WASM EXEC] Trap in sandbox {}: {:?}", sandbox_id, trap);
                None
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WasmExecBridge: loads={}/{} runs={} traps={}",
            self.stats.loads_allowed, self.stats.loads_denied,
            self.stats.runs_allowed, self.stats.trap_count
        );
    }
}
