//! # Prism Extent Allocator
//!
//! Manages on-disk space allocation using extent-based tracking.
//! Supports first-fit, best-fit, and next-fit allocation with
//! coalescing of freed extents.

extern crate alloc;

use alloc::vec::Vec;

/// An extent (contiguous run of blocks on disk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    /// Start block number
    pub start: u64,
    /// Number of blocks
    pub length: u64,
}

impl Extent {
    pub fn new(start: u64, length: u64) -> Self {
        Extent { start, length }
    }

    /// End block (exclusive).
    pub fn end(&self) -> u64 {
        self.start + self.length
    }

    /// Do two extents overlap?
    pub fn overlaps(&self, other: &Extent) -> bool {
        self.start < other.end() && other.start < self.end()
    }

    /// Are two extents adjacent?
    pub fn adjacent(&self, other: &Extent) -> bool {
        self.end() == other.start || other.end() == self.start
    }

    /// Merge with an adjacent extent.
    pub fn merge(&self, other: &Extent) -> Option<Extent> {
        if self.adjacent(other) || self.overlaps(other) {
            let start = self.start.min(other.start);
            let end = self.end().max(other.end());
            Some(Extent::new(start, end - start))
        } else {
            None
        }
    }
}

/// Allocation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocStrategy {
    /// Use the first free extent that fits
    FirstFit,
    /// Use the smallest free extent that fits
    BestFit,
    /// Continue from the last allocation point
    NextFit,
}

/// Allocation result.
#[derive(Debug, Clone)]
pub struct Allocation {
    /// Allocated extent
    pub extent: Extent,
    /// Allocation request ID
    pub request_id: u64,
}

/// Allocator statistics.
#[derive(Debug, Clone, Default)]
pub struct ExtentStats {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub free_extents: u64,
    pub allocations: u64,
    pub frees: u64,
    pub coalesces: u64,
    pub largest_free: u64,
    pub fragmentation_pct: u8,
}

/// The Extent Allocator.
pub struct ExtentAllocator {
    /// Free extents (sorted by start block)
    pub free_list: Vec<Extent>,
    /// Used extents (sorted by start block)
    pub used_list: Vec<Extent>,
    /// Total blocks managed
    pub total_blocks: u64,
    /// Block size (bytes)
    pub block_size: u32,
    /// Allocation strategy
    pub strategy: AllocStrategy,
    /// Next-fit cursor
    next_fit_cursor: usize,
    /// Next request ID
    next_id: u64,
}

impl ExtentAllocator {
    /// Create an allocator for `total_blocks` blocks of `block_size` bytes.
    pub fn new(total_blocks: u64, block_size: u32, strategy: AllocStrategy) -> Self {
        ExtentAllocator {
            free_list: alloc::vec![Extent::new(0, total_blocks)],
            used_list: Vec::new(),
            total_blocks,
            block_size,
            strategy,
            next_fit_cursor: 0,
            next_id: 1,
        }
    }

    /// Allocate `count` contiguous blocks.
    pub fn alloc(&mut self, count: u64) -> Option<Allocation> {
        if count == 0 { return None; }

        let idx = match self.strategy {
            AllocStrategy::FirstFit => self.first_fit(count),
            AllocStrategy::BestFit => self.best_fit(count),
            AllocStrategy::NextFit => self.next_fit(count),
        }?;

        let extent = self.free_list[idx];
        let alloc_extent = Extent::new(extent.start, count);

        // Update free list
        if extent.length == count {
            self.free_list.remove(idx);
        } else {
            self.free_list[idx] = Extent::new(
                extent.start + count,
                extent.length - count,
            );
        }

        // Add to used list (sorted)
        let insert_pos = self.used_list.iter()
            .position(|e| e.start > alloc_extent.start)
            .unwrap_or(self.used_list.len());
        self.used_list.insert(insert_pos, alloc_extent);

        let id = self.next_id;
        self.next_id += 1;

        Some(Allocation {
            extent: alloc_extent,
            request_id: id,
        })
    }

    /// Free an extent (return blocks to the free pool).
    pub fn free(&mut self, extent: Extent) {
        // Remove from used list
        self.used_list.retain(|e| e.start != extent.start);

        // Insert into free list (sorted)
        let insert_pos = self.free_list.iter()
            .position(|e| e.start > extent.start)
            .unwrap_or(self.free_list.len());
        self.free_list.insert(insert_pos, extent);

        // Coalesce with neighbors
        self.coalesce();
    }

    /// First-fit: find the first extent that fits.
    fn first_fit(&self, count: u64) -> Option<usize> {
        self.free_list.iter().position(|e| e.length >= count)
    }

    /// Best-fit: find the smallest extent that fits.
    fn best_fit(&self, count: u64) -> Option<usize> {
        self.free_list.iter()
            .enumerate()
            .filter(|(_, e)| e.length >= count)
            .min_by_key(|(_, e)| e.length)
            .map(|(i, _)| i)
    }

    /// Next-fit: continue from the last allocation point.
    fn next_fit(&mut self, count: u64) -> Option<usize> {
        let len = self.free_list.len();
        if len == 0 { return None; }

        // Search from cursor to end, then wrap around
        for offset in 0..len {
            let idx = (self.next_fit_cursor + offset) % len;
            if self.free_list[idx].length >= count {
                self.next_fit_cursor = (idx + 1) % len;
                return Some(idx);
            }
        }
        None
    }

    /// Coalesce adjacent free extents.
    fn coalesce(&mut self) {
        if self.free_list.len() < 2 { return; }

        let mut i = 0;
        while i + 1 < self.free_list.len() {
            if let Some(merged) = self.free_list[i].merge(&self.free_list[i + 1]) {
                self.free_list[i] = merged;
                self.free_list.remove(i + 1);
                // Don't increment — check if we can merge again
            } else {
                i += 1;
            }
        }
    }

    /// Get allocator stats.
    pub fn stats(&self) -> ExtentStats {
        let used: u64 = self.used_list.iter().map(|e| e.length).sum();
        let free: u64 = self.free_list.iter().map(|e| e.length).sum();
        let largest_free = self.free_list.iter().map(|e| e.length).max().unwrap_or(0);
        let frag = if free > 0 && self.free_list.len() > 1 {
            (100 - (largest_free * 100 / free)) as u8
        } else { 0 };

        ExtentStats {
            total_blocks: self.total_blocks,
            used_blocks: used,
            free_blocks: free,
            free_extents: self.free_list.len() as u64,
            allocations: self.next_id - 1,
            frees: 0,
            coalesces: 0,
            largest_free,
            fragmentation_pct: frag,
        }
    }

    /// Defragment: sort used extents and rebuild free list.
    pub fn defragment(&mut self) {
        self.used_list.sort_by_key(|e| e.start);

        self.free_list.clear();
        let mut cursor = 0u64;

        for used in &self.used_list {
            if used.start > cursor {
                self.free_list.push(Extent::new(cursor, used.start - cursor));
            }
            cursor = used.end();
        }

        if cursor < self.total_blocks {
            self.free_list.push(Extent::new(cursor, self.total_blocks - cursor));
        }
    }
}
