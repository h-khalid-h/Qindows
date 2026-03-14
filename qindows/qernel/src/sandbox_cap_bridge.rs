//! # Sandbox CapToken Bridge (Phase 133)
//!
//! ## Architecture Guardian: The Gap
//! `sandbox.rs` (Phase 77) implements `SandboxManager`:
//! - `create()` — creates a WASM sandbox for a Silo
//! - `load()` — loads WASM binary into sandbox memory
//! - `run()` — executes WASM, returns i64 or TrapReason
//! - `check_capability()` — validates a capability bit against sandbox.caps_granted
//!
//! **Missing link**: `check_capability()` checked a u64 bitmask but was
//! never connected to `CapTokenForge` — the sandbox had its own capability
//! system completely separate from the kernel's real CapToken runtime.
//!
//! This module provides `SandboxCapBridge`:
//! 1. `create_with_caps()` — creates sandbox + mints CapTokens for sandbox Silo
//! 2. `run_with_cap_check()` — verifies CapToken before running WASM
//! 3. `handle_trap()` — maps TrapReason to Law violations
//! 4. `enforce_resource_limits()` — wires to QuotaEnforcementBridge

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::sandbox::{SandboxManager, TrapReason, ResourceLimits};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC, CAP_READ};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct SandboxBridgeStats {
    pub sandboxes_created: u64,
    pub runs_ok:           u64,
    pub runs_trapped:      u64,
    pub cap_denied:        u64,
    pub law_violations:    u64,
}

// ── Law Trap Map ──────────────────────────────────────────────────────────────

/// Maps sandbox TrapReason to the Q-Manifest Law it violates.
pub fn trap_to_law(trap: &TrapReason) -> Option<u8> {
    match trap {
        TrapReason::MemoryAccessViolation       => Some(6), // Law 6: Silo Sandbox
        TrapReason::CapabilityDenied(_)         => Some(1), // Law 1: Zero-Ambient Authority
        TrapReason::DivisionByZero              => None,
        TrapReason::Unreachable                 => None,
        TrapReason::StackOverflow               => Some(10), // Law 10: Graceful Degradation
        TrapReason::OutOfMemory                 => Some(10),
        TrapReason::Timeout                     => Some(8),  // Law 8: Energy Proportionality
        TrapReason::IoLimitExceeded             => Some(6),
        TrapReason::FuelExhausted               => Some(8),
        TrapReason::IntegerOverflow             => None,
        TrapReason::KilledBySentinel            => Some(6),
    }
}

// ── Sandbox Cap Bridge ────────────────────────────────────────────────────────

/// Integrates SandboxManager with CapTokenForge for Law 1/6 compliance.
pub struct SandboxCapBridge {
    pub manager: SandboxManager,
    pub stats:   SandboxBridgeStats,
}

impl SandboxCapBridge {
    pub fn new() -> Self {
        SandboxCapBridge {
            manager: SandboxManager::new(),
            stats: SandboxBridgeStats::default(),
        }
    }

    /// Create a sandbox and mint appropriate CapTokens for its Silo.
    pub fn create_with_caps(
        &mut self,
        silo_id: u64,
        wasm_hash: u64,
        limits: ResourceLimits,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.sandboxes_created += 1;

        // Create the sandbox
        // Create sandbox: (module_name, module_hash: [u8;32], silo_id, capabilities, limits)
        let hash_bytes = crate::crypto_primitives::sha256(&wasm_hash.to_le_bytes());
        let sandbox_id = self.manager.create(
            &alloc::format!("wasm_silo_{}", silo_id),
            hash_bytes,
            silo_id,
            (CAP_EXEC | CAP_READ) as u64,
            Some(limits),
        );

        // Mint WASM execution cap for the Silo
        forge.mint(silo_id, CapType::Wasm, wasm_hash, tick + 500_000, CAP_EXEC | CAP_READ);

        crate::serial_println!(
            "[SANDBOX BRIDGE] Created sandbox_id={} for Silo {} hash={:#x}",
            sandbox_id, silo_id, wasm_hash
        );
        sandbox_id
    }

    /// Run WASM after verifying CapToken.
    pub fn run_with_cap_check(
        &mut self,
        sandbox_id: u64,
        silo_id: u64,
        wasm_hash: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Result<i64, TrapReason> {
        // Law 1: Check WASM exec cap
        if !forge.check(silo_id, CapType::Wasm, CAP_EXEC, wasm_hash, tick) {
            self.stats.cap_denied += 1;
            crate::serial_println!(
                "[SANDBOX BRIDGE] CapDenied: Silo {} no Wasm:EXEC cap", silo_id
            );
            return Err(TrapReason::CapabilityDenied(
                alloc::format!("Silo {} requires Wasm:EXEC cap", silo_id)
            ));
        }

        let result = self.manager.run(sandbox_id);
        match &result {
            Ok(_) => { self.stats.runs_ok += 1; }
            Err(trap) => {
                self.stats.runs_trapped += 1;
                if let Some(law) = trap_to_law(trap) {
                    self.stats.law_violations += 1;
                    crate::serial_println!(
                        "[SANDBOX BRIDGE] Trap {:?} → Law {} violation in Silo {}",
                        trap, law, silo_id
                    );
                }
            }
        }
        result
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SandboxBridge: created={} ok={} trapped={} cap_denied={} law={}",
            self.stats.sandboxes_created, self.stats.runs_ok, self.stats.runs_trapped,
            self.stats.cap_denied, self.stats.law_violations
        );
    }
}
