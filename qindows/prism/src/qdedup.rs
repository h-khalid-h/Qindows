//! # Q-Dedup — Content-Level Deduplication Across Silos
//!
//! Identifies and deduplicates identical content blocks
//! across Silos to save storage (Section 3.14).
//!
//! Features:
//! - Block-level dedup (variable-length chunking)
//! - Content-addressed: SHA-256 fingerprint per block
//! - Reference counting (block freed when refcount drops to 0)
//! - Cross-Silo dedup (shared blocks save space globally)
//! - Inline vs deferred dedup modes

extern crate alloc;

use alloc::collections::BTreeMap;

/// A deduplicated block.
#[derive(Debug, Clone)]
pub struct DedupBlock {
    pub hash: [u8; 32],
    pub size: u64,
    pub refcount: u64,
    pub stored_at: u64,
}

/// Dedup mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupMode {
    Inline,   // Dedup at write time
    Deferred, // Dedup in background
    Disabled,
}

/// Dedup statistics.
#[derive(Debug, Clone, Default)]
pub struct DedupStats {
    pub blocks_stored: u64,
    pub blocks_deduped: u64,
    pub bytes_logical: u64,
    pub bytes_physical: u64,
    pub ref_increments: u64,
    pub ref_decrements: u64,
    pub blocks_freed: u64,
}

/// The Q-Dedup Engine.
pub struct QDedup {
    pub blocks: BTreeMap<[u8; 32], DedupBlock>,
    /// Object → list of block hashes
    pub object_blocks: BTreeMap<u64, alloc::vec::Vec<[u8; 32]>>,
    pub mode: DedupMode,
    pub min_block_size: u64,
    pub max_block_size: u64,
    pub stats: DedupStats,
}

impl QDedup {
    pub fn new() -> Self {
        QDedup {
            blocks: BTreeMap::new(),
            object_blocks: BTreeMap::new(),
            mode: DedupMode::Inline,
            min_block_size: 4096,
            max_block_size: 65536,
            stats: DedupStats::default(),
        }
    }

    /// Store a block (dedup if hash exists).
    pub fn store_block(&mut self, hash: [u8; 32], size: u64, now: u64) -> bool {
        if let Some(block) = self.blocks.get_mut(&hash) {
            block.refcount += 1;
            self.stats.blocks_deduped += 1;
            self.stats.ref_increments += 1;
            self.stats.bytes_logical += size;
            return true; // Deduplicated
        }

        self.blocks.insert(hash, DedupBlock {
            hash, size, refcount: 1, stored_at: now,
        });
        self.stats.blocks_stored += 1;
        self.stats.bytes_logical += size;
        self.stats.bytes_physical += size;
        false // New block
    }

    /// Map an object to its blocks.
    pub fn map_object(&mut self, oid: u64, block_hashes: alloc::vec::Vec<[u8; 32]>) {
        self.object_blocks.insert(oid, block_hashes);
    }

    /// Release blocks when an object is deleted.
    pub fn release_object(&mut self, oid: u64) {
        if let Some(hashes) = self.object_blocks.remove(&oid) {
            // First pass: decrement refcounts and collect blocks to free
            let mut to_free = alloc::vec::Vec::new();
            for hash in &hashes {
                if let Some(block) = self.blocks.get_mut(hash) {
                    block.refcount = block.refcount.saturating_sub(1);
                    self.stats.ref_decrements += 1;
                    if block.refcount == 0 {
                        to_free.push((*hash, block.size));
                    }
                }
            }
            // Second pass: remove freed blocks
            for (hash, size) in to_free {
                self.blocks.remove(&hash);
                self.stats.blocks_freed += 1;
                self.stats.bytes_physical = self.stats.bytes_physical.saturating_sub(size);
            }
        }
    }

    /// Get dedup ratio.
    pub fn dedup_ratio(&self) -> f32 {
        if self.stats.bytes_logical > 0 {
            1.0 - (self.stats.bytes_physical as f32 / self.stats.bytes_logical as f32)
        } else {
            0.0
        }
    }

    /// Get space saved.
    pub fn space_saved(&self) -> u64 {
        self.stats.bytes_logical.saturating_sub(self.stats.bytes_physical)
    }
}
