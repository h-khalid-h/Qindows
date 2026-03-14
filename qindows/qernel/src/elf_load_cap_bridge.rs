//! # ELF Load Cap Bridge (Phase 214)
//!
//! ## Architecture Guardian: The Gap
//! `elf.rs` implements ELF binary parsing and loading.
//! ELF loading is the critical path for new process/Silo creation.
//!
//! **Missing link**: ELF binary loading (elf.rs) could proceed without
//! verifying the binary was signed or hash-matched. Any Silo could
//! load an unsigned ELF into a new Silo, defeating secure boot (Law 2).
//!
//! This module provides `ElfLoadCapBridge`:
//! Admin:EXEC + secure_boot hash check before ELF loading is authorized.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct ElfLoadCapStats {
    pub loads_allowed: u64,
    pub loads_denied:  u64,
    pub hash_mismatches: u64,
}

pub struct ElfLoadCapBridge {
    pub stats: ElfLoadCapStats,
}

impl ElfLoadCapBridge {
    pub fn new() -> Self {
        ElfLoadCapBridge { stats: ElfLoadCapStats::default() }
    }

    /// Authorize ELF load — requires Admin:EXEC cap + expected hash match.
    pub fn authorize_load(
        &mut self,
        silo_id: u64,
        elf_hash: &[u8; 32],
        expected_hash: &[u8; 32],
        forge: &mut CapTokenForge,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.loads_denied += 1;
            crate::serial_println!("[ELF LOAD] Silo {} load denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        if elf_hash != expected_hash {
            self.stats.hash_mismatches += 1;
            self.stats.loads_denied += 1;
            audit.log_law_violation(2u8, silo_id, tick);
            crate::serial_println!("[ELF LOAD] Silo {} hash MISMATCH — Law 2 audit", silo_id);
            return false;
        }
        self.stats.loads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ElfLoadBridge: allowed={} denied={} hash_mismatch={}",
            self.stats.loads_allowed, self.stats.loads_denied, self.stats.hash_mismatches
        );
    }
}
