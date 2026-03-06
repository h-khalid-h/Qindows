//! # Q-Extent — Extent-Based Block Allocator
//!
//! Manages contiguous block extents for Prism object storage.
//! Uses a best-fit allocator with free-list coalescing
//! for minimal fragmentation (Section 3.37).
//!
//! Features:
//! - Extent allocation (best-fit, first-fit)
//! - Free-list with coalescing
//! - Per-Silo allocation tracking
//! - Reservation (pre-allocate without commit)
//! - Statistics and free-space reporting

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// An allocated extent.
#[derive(Debug, Clone)]
pub struct Extent {
    pub start: u64,
    pub length: u32,
    pub silo_id: u64,
    pub oid: u64,
    pub reserved: bool,
}

/// Allocation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocStrategy {
    FirstFit,
    BestFit,
}

/// Extent allocator statistics.
#[derive(Debug, Clone, Default)]
pub struct ExtentStats {
    pub allocs: u64,
    pub frees: u64,
    pub coalesces: u64,
    pub total_allocated: u64,
    pub total_free: u64,
    pub fragmentation_pct: f64,
}

/// The Q-Extent Allocator.
pub struct QExtent {
    /// Free extents (start → length)
    pub free_list: BTreeMap<u64, u32>,
    /// Allocated extents (start → extent)
    pub allocated: BTreeMap<u64, Extent>,
    pub strategy: AllocStrategy,
    pub total_blocks: u64,
    pub stats: ExtentStats,
}

impl QExtent {
    pub fn new(total_blocks: u64, strategy: AllocStrategy) -> Self {
        let mut free_list = BTreeMap::new();
        free_list.insert(0, total_blocks as u32);

        QExtent {
            free_list, allocated: BTreeMap::new(),
            strategy, total_blocks,
            stats: ExtentStats { total_free: total_blocks, ..Default::default() },
        }
    }

    /// Allocate an extent.
    pub fn allocate(&mut self, blocks: u32, silo_id: u64, oid: u64) -> Option<u64> {
        let start = match self.strategy {
            AllocStrategy::FirstFit => self.find_first_fit(blocks)?,
            AllocStrategy::BestFit => self.find_best_fit(blocks)?,
        };

        // Remove from free list
        let free_len = self.free_list.remove(&start)?;

        // If the free extent is larger than needed, put remainder back
        if free_len > blocks {
            self.free_list.insert(start + blocks as u64, free_len - blocks);
        }

        self.allocated.insert(start, Extent {
            start, length: blocks, silo_id, oid, reserved: false,
        });

        self.stats.allocs += 1;
        self.stats.total_allocated += blocks as u64;
        self.stats.total_free -= blocks as u64;
        self.update_fragmentation();

        Some(start)
    }

    /// Free an extent.
    pub fn free(&mut self, start: u64) -> bool {
        let extent = match self.allocated.remove(&start) {
            Some(e) => e,
            None => return false,
        };

        self.free_list.insert(start, extent.length);
        self.stats.frees += 1;
        self.stats.total_allocated -= extent.length as u64;
        self.stats.total_free += extent.length as u64;

        self.coalesce(start);
        self.update_fragmentation();
        true
    }

    fn find_first_fit(&self, blocks: u32) -> Option<u64> {
        self.free_list.iter()
            .find(|(_, &len)| len >= blocks)
            .map(|(&start, _)| start)
    }

    fn find_best_fit(&self, blocks: u32) -> Option<u64> {
        self.free_list.iter()
            .filter(|(_, &len)| len >= blocks)
            .min_by_key(|(_, &len)| len)
            .map(|(&start, _)| start)
    }

    /// Coalesce adjacent free extents.
    fn coalesce(&mut self, start: u64) {
        let len = match self.free_list.get(&start) {
            Some(&l) => l,
            None => return,
        };

        // Merge with next
        let next_start = start + len as u64;
        if let Some(&next_len) = self.free_list.get(&next_start) {
            self.free_list.remove(&next_start);
            if let Some(l) = self.free_list.get_mut(&start) {
                *l += next_len;
            }
            self.stats.coalesces += 1;
        }

        // Merge with previous
        let prev: Option<(u64, u32)> = self.free_list.range(..start)
            .rev()
            .next()
            .map(|(&s, &l)| (s, l));

        if let Some((prev_start, prev_len)) = prev {
            if prev_start + prev_len as u64 == start {
                let cur_len = self.free_list.remove(&start).unwrap_or(0);
                if let Some(l) = self.free_list.get_mut(&prev_start) {
                    *l += cur_len;
                }
                self.stats.coalesces += 1;
            }
        }
    }

    fn update_fragmentation(&mut self) {
        let free_regions = self.free_list.len() as f64;
        if self.stats.total_free == 0 {
            self.stats.fragmentation_pct = 0.0;
        } else {
            self.stats.fragmentation_pct = ((free_regions - 1.0) / free_regions * 100.0).max(0.0);
        }
    }

    /// Available free blocks.
    pub fn available(&self) -> u64 {
        self.stats.total_free
    }
}
