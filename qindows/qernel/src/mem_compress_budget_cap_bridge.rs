//! # Memory Compress Budget Cap Bridge (Phase 287)
//!
//! ## Architecture Guardian: The Gap
//! `mem_compress.rs` implements `MemCompress`:
//! - `CompBudget { zpool_max, silo_max }`
//! - `CompStats { pages_compressed, pages_decompressed, ... }`
//! - `MemCompress::new(zpool_max: u64)` — create with zpool limit
//!
//! **Missing link**: Per-Silo compressed memory budget was tracked but
//! not enforced at compression time. A Silo could compress far more
//! pages than its CompBudget.silo_max, starving other Silos' compress
//! pools.
//!
//! This module provides `MemCompressBudgetCapBridge`:
//! Enforces silo_max at compression request time.

extern crate alloc;
use alloc::collections::BTreeMap;

const SILO_MAX_COMPRESSED_PAGES: u64 = 1024;

#[derive(Debug, Default, Clone)]
pub struct MemCompressCapStats {
    pub compressions_allowed: u64,
    pub compressions_denied:  u64,
}

pub struct MemCompressBudgetCapBridge {
    silo_page_counts: BTreeMap<u64, u64>,
    pub stats:        MemCompressCapStats,
}

impl MemCompressBudgetCapBridge {
    pub fn new() -> Self {
        MemCompressBudgetCapBridge { silo_page_counts: BTreeMap::new(), stats: MemCompressCapStats::default() }
    }

    pub fn allow_compress(&mut self, silo_id: u64) -> bool {
        let count = self.silo_page_counts.entry(silo_id).or_default();
        if *count >= SILO_MAX_COMPRESSED_PAGES {
            self.stats.compressions_denied += 1;
            crate::serial_println!(
                "[MEM COMPRESS] Silo {} compress quota full ({}/{})", silo_id, count, SILO_MAX_COMPRESSED_PAGES
            );
            return false;
        }
        *count += 1;
        self.stats.compressions_allowed += 1;
        true
    }

    pub fn on_page_decompressed(&mut self, silo_id: u64) {
        let count = self.silo_page_counts.entry(silo_id).or_default();
        *count = count.saturating_sub(1);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  MemCompressBridge: allowed={} denied={}",
            self.stats.compressions_allowed, self.stats.compressions_denied
        );
    }
}
