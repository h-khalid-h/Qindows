//! # ELF Binary Load Cap Bridge (Phase 282)
//!
//! ## Architecture Guardian: The Gap
//! `elf.rs` implements ELF binary loading:
//! - `Elf64Header { e_type: ElfType, e_phoff, e_shoff, e_phnum, e_shnum, ... }`
//! - `Elf64Phdr { p_type: PhType, p_offset, p_vaddr, p_filesz, p_memsz, ... }`
//! - `PhType` — Load, Dynamic, Interp, Note, ...
//!
//! **Missing link**: ELF LOAD sections had no size cap. A malicious ELF
//! binary with extremely large `p_memsz` (up to 2^64) could block the
//! kernel in zero-page allocation for hours, causing starvation.
//!
//! This module provides `ElfBinaryLoadCapBridge`:
//! Max 512 MiB total LOAD segment memsz per ELF binary.

extern crate alloc;

const MAX_TOTAL_MEMSZ_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB

#[derive(Debug, Default, Clone)]
pub struct ElfLoadCapStats {
    pub loads_allowed: u64,
    pub loads_denied:  u64,
}

pub struct ElfBinaryLoadCapBridge {
    pub stats: ElfLoadCapStats,
}

impl ElfBinaryLoadCapBridge {
    pub fn new() -> Self {
        ElfBinaryLoadCapBridge { stats: ElfLoadCapStats::default() }
    }

    pub fn authorize_load(&mut self, total_memsz: u64, silo_id: u64) -> bool {
        if total_memsz > MAX_TOTAL_MEMSZ_BYTES {
            self.stats.loads_denied += 1;
            crate::serial_println!(
                "[ELF] Silo {} binary memsz {} bytes exceeds {} MiB cap",
                silo_id, total_memsz, MAX_TOTAL_MEMSZ_BYTES / (1024*1024)
            );
            return false;
        }
        self.stats.loads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ElfLoadCapBridge: allowed={} denied={}", self.stats.loads_allowed, self.stats.loads_denied
        );
    }
}
