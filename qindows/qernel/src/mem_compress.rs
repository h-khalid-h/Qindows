//! # Memory Compressor — zswap-Style Page Compression
//!
//! Compresses memory pages before swapping to disk, reducing
//! I/O pressure and improving responsiveness (Section 1.7).
//!
//! Features:
//! - In-memory compressed page pool (zpool)
//! - LZ4 fast compression for hot eviction path
//! - Per-Silo compression budgets
//! - Writeback to swap when zpool is full
//! - Compression ratio tracking

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Compression state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompState {
    Uncompressed,
    Compressed,
    WrittenBack,
}

/// A compressed page entry.
#[derive(Debug, Clone)]
pub struct CompPage {
    pub pfn: u64,
    pub silo_id: u64,
    pub original_size: u32,
    pub compressed_size: u32,
    pub state: CompState,
    pub stored_at: u64,
}

/// Per-Silo compression budget.
#[derive(Debug, Clone)]
pub struct CompBudget {
    pub silo_id: u64,
    pub max_pages: u64,
    pub pages_used: u64,
    pub bytes_saved: u64,
}

/// Compression statistics.
#[derive(Debug, Clone, Default)]
pub struct CompStats {
    pub pages_compressed: u64,
    pub pages_decompressed: u64,
    pub pages_written_back: u64,
    pub bytes_original: u64,
    pub bytes_compressed: u64,
    pub compressions_failed: u64,
}

/// The Memory Compressor.
pub struct MemCompress {
    pub zpool: BTreeMap<u64, CompPage>,
    pub budgets: BTreeMap<u64, CompBudget>,
    pub zpool_max: u64,
    pub zpool_used: u64,
    pub min_ratio: f32,
    pub stats: CompStats,
}

impl MemCompress {
    pub fn new(zpool_max: u64) -> Self {
        MemCompress {
            zpool: BTreeMap::new(),
            budgets: BTreeMap::new(),
            zpool_max,
            zpool_used: 0,
            min_ratio: 0.5, // Don't compress if ratio > 50%
            stats: CompStats::default(),
        }
    }

    /// Set per-Silo budget.
    pub fn set_budget(&mut self, silo_id: u64, max_pages: u64) {
        self.budgets.entry(silo_id).or_insert(CompBudget {
            silo_id, max_pages, pages_used: 0, bytes_saved: 0,
        }).max_pages = max_pages;
    }

    /// Compress a page into the zpool.
    pub fn compress(&mut self, pfn: u64, silo_id: u64, original_size: u32, compressed_size: u32, now: u64) -> Result<(), &'static str> {
        // Check if compression is worthwhile
        if compressed_size as f32 / original_size as f32 > self.min_ratio {
            self.stats.compressions_failed += 1;
            return Err("Compression ratio too poor");
        }

        // Check zpool capacity
        if self.zpool_used + compressed_size as u64 > self.zpool_max {
            return Err("Zpool full");
        }

        // Check per-Silo budget
        if let Some(budget) = self.budgets.get(&silo_id) {
            if budget.pages_used >= budget.max_pages {
                return Err("Silo compression budget exceeded");
            }
        }

        self.zpool.insert(pfn, CompPage {
            pfn, silo_id, original_size, compressed_size,
            state: CompState::Compressed, stored_at: now,
        });

        self.zpool_used += compressed_size as u64;
        let saved = (original_size - compressed_size) as u64;

        if let Some(budget) = self.budgets.get_mut(&silo_id) {
            budget.pages_used += 1;
            budget.bytes_saved += saved;
        }

        self.stats.pages_compressed += 1;
        self.stats.bytes_original += original_size as u64;
        self.stats.bytes_compressed += compressed_size as u64;
        Ok(())
    }

    /// Decompress a page from the zpool.
    pub fn decompress(&mut self, pfn: u64) -> Result<u32, &'static str> {
        let page = self.zpool.remove(&pfn).ok_or("Page not in zpool")?;
        self.zpool_used = self.zpool_used.saturating_sub(page.compressed_size as u64);

        if let Some(budget) = self.budgets.get_mut(&page.silo_id) {
            budget.pages_used = budget.pages_used.saturating_sub(1);
        }

        self.stats.pages_decompressed += 1;
        Ok(page.original_size)
    }

    /// Writeback oldest pages to swap when zpool pressure is high.
    pub fn writeback(&mut self, count: usize) -> Vec<u64> {
        let mut written = Vec::new();
        let pfns: Vec<u64> = self.zpool.keys().copied().take(count).collect();

        for pfn in pfns {
            if let Some(page) = self.zpool.get_mut(&pfn) {
                if page.state == CompState::Compressed {
                    page.state = CompState::WrittenBack;
                    self.stats.pages_written_back += 1;
                    written.push(pfn);
                }
            }
        }
        written
    }

    /// Get compression ratio.
    pub fn ratio(&self) -> f32 {
        if self.stats.bytes_original > 0 {
            self.stats.bytes_compressed as f32 / self.stats.bytes_original as f32
        } else {
            1.0
        }
    }
}
