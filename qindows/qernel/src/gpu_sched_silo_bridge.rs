//! # GPU Scheduler Silo Bridge (Phase 159)
//!
//! ## Architecture Guardian: The Gap
//! `gpu_sched.rs` — GPU compute work was never gated by CapToken.
//! CapType has: None/Prism/Aether/Ipc/Network/Admin/Wasm/Collab/Synapse/Energy
//! (no GpuCompute — GPU work uses Energy cap as the governing token).
//!
//! This module provides `GpuSchedSiloBridge`:
//! Silo must hold Energy:EXEC cap to submit GPU compute workloads.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct GpuBridgeStats {
    pub allowed: u64,
    pub denied:  u64,
}

/// Gates GPU compute submissions behind Energy:EXEC CapToken.
pub struct GpuSchedSiloBridge {
    pub stats: GpuBridgeStats,
}

impl GpuSchedSiloBridge {
    pub fn new() -> Self {
        GpuSchedSiloBridge { stats: GpuBridgeStats::default() }
    }

    /// Gate a GPU workload submission. Energy:EXEC required (GPU is an energy resource).
    pub fn submit_with_cap_check(
        &mut self,
        silo_id: u64,
        workload_size_us: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Energy, CAP_EXEC, 0, tick) {
            self.stats.denied += 1;
            crate::serial_println!(
                "[GPU BRIDGE] Silo {} denied GPU workload — no Energy:EXEC cap", silo_id
            );
            return false;
        }
        self.stats.allowed += 1;
        let _ = workload_size_us;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!("  GpuBridge: allowed={} denied={}", self.stats.allowed, self.stats.denied);
    }
}
