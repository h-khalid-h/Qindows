//! # Silo Fork CoW Bridge (Phase 182)
//!
//! ## Architecture Guardian: The Gap
//! `q_silo_fork.rs` implements `SiloForkEngine`:
//! - `fork(parent_silo_id, policy: ForkPolicy, delegated_caps: Vec<u64>, tick)` → child_silo_id: u64
//! - `handle_cow_fault(silo_id, phys_frame)` → Option<u64> (new private frame)
//! - Internally manages `CoWPageRecord` with `sharers: BTreeMap<u64, bool>` per frame
//!
//! `ForkPolicy` variants: Copy, Lazy (CoW), Share, ReadOnlySnapshot
//!
//! **Missing link**: `SiloForkEngine::fork()` was never guarded by CapTokens.
//! A Silo without Network:EXEC or minimum capability could still fork,
//! creating unlimited child Silos and exhausting kernel fork table memory.
//!
//! This module provides `SiloForkCoWBridge`:
//! 1. `fork_with_cap_check()` — Network:EXEC cap required, forwards to SiloForkEngine::fork()
//! 2. `handle_cow_fault()` — delegate fault to real CoW engine

extern crate alloc;
use alloc::vec::Vec;

use crate::q_silo_fork::{SiloForkEngine, ForkPolicy};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct ForkCoWStats {
    pub forks_allowed: u64,
    pub forks_denied:  u64,
    pub cow_faults:    u64,
}

pub struct SiloForkCoWBridge {
    pub engine: SiloForkEngine,
    pub stats:  ForkCoWStats,
}

impl SiloForkCoWBridge {
    pub fn new() -> Self {
        SiloForkCoWBridge { engine: SiloForkEngine::new(), stats: ForkCoWStats::default() }
    }

    /// Fork a Silo — requires Network:EXEC cap (Silo processes are mesh citizens).
    pub fn fork_with_cap_check(
        &mut self,
        parent_silo: u64,
        policy: ForkPolicy,
        delegated_caps: Vec<u64>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(parent_silo, CapType::Network, CAP_EXEC, 0, tick) {
            self.stats.forks_denied += 1;
            crate::serial_println!(
                "[FORK CoW] Silo {} fork denied — no Network:EXEC cap", parent_silo
            );
            return None;
        }
        self.stats.forks_allowed += 1;
        let child_id = self.engine.fork(parent_silo, policy, delegated_caps, tick);
        crate::serial_println!("[FORK CoW] Silo {} → child Silo {} forked", parent_silo, child_id);
        Some(child_id)
    }

    /// Handle a CoW fault — delegate to SiloForkEngine.
    pub fn handle_cow_fault(&mut self, silo_id: u64, phys_frame: u64) -> Option<u64> {
        self.stats.cow_faults += 1;
        self.engine.handle_cow_fault(silo_id, phys_frame)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ForkCoWBridge: allowed={} denied={} cow_faults={}",
            self.stats.forks_allowed, self.stats.forks_denied, self.stats.cow_faults
        );
    }
}
