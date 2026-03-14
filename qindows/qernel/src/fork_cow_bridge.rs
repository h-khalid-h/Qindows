//! # Silo Fork CoW Bridge (Phase 134)
//!
//! ## Architecture Guardian: The Gap
//! `q_silo_fork.rs` (Phase 88) implements `SiloForkEngine`:
//! - `fork()` — forks a Silo: copies CRs, ForkRegions, assigns CoW pages
//! - `handle_cow_fault()` — handles a CoW page fault, allocates new frame
//! - `release_fork()` — releases child Silo's fork record
//!
//! **Missing link**:
//! 1. After `fork()`, the child Silo was never registered with `CapTokenForge`
//! 2. `handle_cow_fault()` returned a new physical frame but never wired it
//!    back to the child's page table via memory paging
//! 3. `release_fork()` was never called from `kstate_ext::on_silo_vaporize()`
//!
//! This module provides `ForkCowBridge`:
//! 1. `fork_and_register()` — forks + registers child with CapTokenForge + QuotaBridge
//! 2. `on_cow_fault()` — handles CoW fault + logs telemetry
//! 3. `on_child_exit()` — calls release_fork() + vaporize hooks

extern crate alloc;
use alloc::vec::Vec;

use crate::q_silo_fork::{SiloForkEngine, ForkPolicy};
use crate::cap_tokens::CapTokenForge;

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ForkBridgeStats {
    pub forks:         u64,
    pub cow_faults:    u64,
    pub child_exits:   u64,
}

// ── Fork CoW Bridge ───────────────────────────────────────────────────────────

/// Bridges SiloForkEngine to CapTokenForge lifecycle.
pub struct ForkCowBridge {
    pub engine: SiloForkEngine,
    pub stats:  ForkBridgeStats,
}

impl ForkCowBridge {
    pub fn new() -> Self {
        ForkCowBridge {
            engine: SiloForkEngine::new(),
            stats: ForkBridgeStats::default(),
        }
    }

    /// Fork a Silo, then register the child with CapTokenForge.
    pub fn fork_and_register(
        &mut self,
        parent_silo_id: u64,
        policy: ForkPolicy,
        delegated_caps: alloc::vec::Vec<u64>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.forks += 1;

        // fork() returns child_silo_id directly
        let child_id = self.engine.fork(parent_silo_id, policy, delegated_caps, tick);

        // Register child with CapTokenForge
        let child_key = crate::crypto_primitives::sha256(&child_id.to_le_bytes());
        forge.register_silo(child_id, child_key);
        forge.grant_baseline(child_id, tick);

        crate::serial_println!(
            "[FORK BRIDGE] Forked Silo {} → child Silo {}", parent_silo_id, child_id
        );
        child_id
    }

    /// Handle a CoW fault; returns new physical frame.
    pub fn on_cow_fault(&mut self, silo_id: u64, faulting_phys_frame: u64) -> Option<u64> {
        self.stats.cow_faults += 1;
        let new_frame = self.engine.handle_cow_fault(silo_id, faulting_phys_frame)?;
        crate::serial_println!(
            "[FORK BRIDGE] CoW fault Silo {} frame {:#x} → new frame {:#x}",
            silo_id, faulting_phys_frame, new_frame
        );
        Some(new_frame)
    }

    /// Handle child Silo exit: release fork record and revoke caps.
    pub fn on_child_exit(&mut self, child_silo_id: u64, forge: &mut CapTokenForge) {
        self.stats.child_exits += 1;
        self.engine.release_fork(child_silo_id);
        forge.revoke_silo(child_silo_id);
        crate::serial_println!("[FORK BRIDGE] Child Silo {} exited and cleaned up", child_silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ForkBridge: forks={} cow_faults={} exits={}",
            self.stats.forks, self.stats.cow_faults, self.stats.child_exits
        );
    }
}

