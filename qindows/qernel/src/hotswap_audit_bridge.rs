//! # HotSwap Audit Bridge (Phase 178)
//!
//! ## Architecture Guardian: The Gap
//! `hotswap_verifier.rs` implements `HotSwapVerifier`:
//! - `stage(module_name, patch_bytes, new_entry_point, tick)` → (patch_id: u64, oid: [u8;32])
//! - `verify(patch_id, patch_bytes)` → VerifyResult (Ok/NotStaged/HashMismatch/OidMismatch)
//! - `apply(patch_id, module_name, tick)` → Result<(), &str>
//! - `rollback_with_audit(patch_id, ...)` — rollback with audit log
//!
//! **Missing link**: `apply()` was called without Admin:EXEC cap check
//! and without going through stage→verify→apply sequence. Live binary
//! updates bypassed integrity verification (Law 2 violation).
//!
//! This module provides `HotSwapAuditBridge`:
//! `begin_hotswap()` — Admin:EXEC check + stage → verify → apply pipeline.

extern crate alloc;

use crate::hotswap_verifier::{HotSwapVerifier, VerifyResult};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct HotSwapBridgeStats {
    pub attempted:  u64,
    pub completed:  u64,
    pub rollbacks:  u64,
    pub cap_denied: u64,
}

pub struct HotSwapAuditBridge {
    pub verifier: HotSwapVerifier,
    pub stats:    HotSwapBridgeStats,
}

impl HotSwapAuditBridge {
    pub fn new() -> Self {
        HotSwapAuditBridge { verifier: HotSwapVerifier::new(), stats: HotSwapBridgeStats::default() }
    }

    /// Stage, verify, and apply a hot-swap — requires Admin:EXEC cap (Law 2).
    pub fn begin_hotswap(
        &mut self,
        silo_id: u64,
        module_name: &str,
        patch_bytes: &[u8],
        new_entry_point: u64,
        forge: &mut CapTokenForge,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.cap_denied += 1;
            crate::serial_println!("[HOTSWAP] Silo {} denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.attempted += 1;

        // Stage the patch
        let (patch_id, _oid) = self.verifier.stage(module_name, patch_bytes, new_entry_point, tick);

        // Verify integrity
        let result = self.verifier.verify(patch_id, patch_bytes);
        if result != VerifyResult::Ok {
            self.stats.rollbacks += 1;
            crate::serial_println!("[HOTSWAP] Verify failed ({:?}) — aborting", result);
            return false;
        }

        // Apply atomically
        match self.verifier.apply(patch_id, module_name, tick) {
            Ok(()) => {
                self.stats.completed += 1;
                audit.log_hotswap(module_name, &[0u8; 32], tick);
                crate::serial_println!("[HOTSWAP] Module {} updated (Law 2: binary integrity)", module_name);
                true
            }
            Err(e) => {
                self.stats.rollbacks += 1;
                crate::serial_println!("[HOTSWAP] Apply failed: {} — rolling back", e);
                false
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  HotSwapBridge: attempted={} completed={} rollbacks={} denied={}",
            self.stats.attempted, self.stats.completed, self.stats.rollbacks, self.stats.cap_denied
        );
    }
}
