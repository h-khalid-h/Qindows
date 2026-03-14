//! # Memory Compression Silo Quota Bridge (Phase 208)
//!
//! ## Architecture Guardian: The Gap
//! `mem_compress.rs` implements `MemCompress`:
//! - `set_budget(silo_id, max_pages)` — set per-Silo compression budget
//! - `compress(pfn, silo_id, original_size, compressed_size, now)` → Result<(), &str>
//!
//! **Missing link**: `set_budget()` could grant unlimited compression pages.
//! Zpool exhaustion from one Silo would prevent all other Silos from
//! compressing memory, causing OOM kills.
//!
//! This module provides `MemCompressSiloQuotaBridge`:
//! Caps per-Silo compression budget at 2048 pages to prevent monopolization.

extern crate alloc;

use crate::mem_compress::MemCompress;

const MAX_COMPRESS_PAGES_PER_SILO: u64 = 2048;

#[derive(Debug, Default, Clone)]
pub struct MemCompressQuotaStats {
    pub budgets_set: u64,
    pub caps_applied: u64,
    pub compressions: u64,
}

pub struct MemCompressSiloQuotaBridge {
    pub compress: MemCompress,
    pub stats:    MemCompressQuotaStats,
}

impl MemCompressSiloQuotaBridge {
    pub fn new(zpool_max: u64) -> Self {
        MemCompressSiloQuotaBridge { compress: MemCompress::new(zpool_max), stats: MemCompressQuotaStats::default() }
    }

    /// Set per-Silo compression budget — capped at 2048 pages.
    pub fn set_budget(&mut self, silo_id: u64, requested: u64) {
        self.stats.budgets_set += 1;
        let actual = if requested > MAX_COMPRESS_PAGES_PER_SILO {
            self.stats.caps_applied += 1;
            crate::serial_println!("[MEM COMPRESS] Silo {} budget {} capped to {}", silo_id, requested, MAX_COMPRESS_PAGES_PER_SILO);
            MAX_COMPRESS_PAGES_PER_SILO
        } else {
            requested
        };
        self.compress.set_budget(silo_id, actual);
    }

    pub fn compress(&mut self, pfn: u64, silo_id: u64, original_size: u32, compressed_size: u32, now: u64) -> bool {
        self.stats.compressions += 1;
        self.compress.compress(pfn, silo_id, original_size, compressed_size, now).is_ok()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  MemCompressBridge: budgets={} capped={} compressions={}",
            self.stats.budgets_set, self.stats.caps_applied, self.stats.compressions
        );
    }
}
