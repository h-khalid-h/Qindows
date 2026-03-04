//! # Prism Deduplication Engine
//!
//! Content-addressable deduplication using rolling hash (Rabin fingerprint).
//! Every chunk of data stored in Prism is hashed — if two files share
//! identical chunks, only one copy is stored. This is transparent
//! to users and saves massive storage.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Chunk hash — 256-bit content-derived address.
pub type ChunkHash = [u8; 32];

/// A deduplicated chunk reference.
#[derive(Debug, Clone)]
pub struct ChunkRef {
    /// Content hash (the address)
    pub hash: ChunkHash,
    /// Reference count (how many objects point here)
    pub ref_count: u32,
    /// Chunk size in bytes
    pub size: u32,
    /// Block address on disk
    pub block_addr: u64,
    /// Is this chunk compressed?
    pub compressed: bool,
}

/// Dedup statistics.
#[derive(Debug, Clone, Default)]
pub struct DedupStats {
    /// Total chunks stored
    pub total_chunks: u64,
    /// Unique chunks (actual disk usage)
    pub unique_chunks: u64,
    /// Duplicate chunks eliminated
    pub dedup_chunks: u64,
    /// Total logical bytes
    pub logical_bytes: u64,
    /// Actual physical bytes on disk
    pub physical_bytes: u64,
}

impl DedupStats {
    /// Deduplication ratio (e.g., 2.5x means 60% savings).
    pub fn ratio(&self) -> f64 {
        if self.physical_bytes == 0 {
            return 1.0;
        }
        self.logical_bytes as f64 / self.physical_bytes as f64
    }

    /// Bytes saved by deduplication.
    pub fn bytes_saved(&self) -> u64 {
        self.logical_bytes.saturating_sub(self.physical_bytes)
    }
}

/// Content-defined chunking parameters.
const MIN_CHUNK_SIZE: usize = 4 * 1024;       // 4 KiB minimum
const MAX_CHUNK_SIZE: usize = 64 * 1024;      // 64 KiB maximum
const TARGET_CHUNK_SIZE: usize = 16 * 1024;   // 16 KiB target average

/// Rabin fingerprint polynomial (irreducible over GF(2)).
const RABIN_POLY: u64 = 0x3DA3358B4DC173;

/// The Deduplication Engine.
pub struct DedupEngine {
    /// Chunk index: hash → chunk reference
    pub index: BTreeMap<ChunkHash, ChunkRef>,
    /// Statistics
    pub stats: DedupStats,
    /// Next available block address
    next_block: u64,
}

impl DedupEngine {
    pub fn new() -> Self {
        DedupEngine {
            index: BTreeMap::new(),
            stats: DedupStats::default(),
            next_block: 0,
        }
    }

    /// Split data into content-defined chunks using Rabin fingerprinting.
    pub fn chunk_data(&self, data: &[u8]) -> Vec<(usize, usize)> {
        let mut chunks = Vec::new();
        let mut start = 0;
        let mut fingerprint: u64 = 0;

        let mask = TARGET_CHUNK_SIZE as u64 - 1; // Power-of-2 mask

        let mut i = start;
        while i < data.len() {
            // Rolling hash update
            fingerprint = fingerprint.wrapping_mul(256).wrapping_add(data[i] as u64);
            fingerprint ^= RABIN_POLY;

            let chunk_len = i - start + 1;

            // Check if we should cut here
            let should_cut = (chunk_len >= MIN_CHUNK_SIZE && (fingerprint & mask) == 0)
                || chunk_len >= MAX_CHUNK_SIZE;

            if should_cut {
                chunks.push((start, chunk_len));
                start = i + 1;
                fingerprint = 0;
            }

            i += 1;
        }

        // Final chunk (if any data remains)
        if start < data.len() {
            chunks.push((start, data.len() - start));
        }

        chunks
    }

    /// Compute a content hash for a chunk.
    pub fn hash_chunk(&self, data: &[u8]) -> ChunkHash {
        // Simple hash for now — production would use BLAKE3
        let mut hash = [0u8; 32];
        let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis

        for &byte in data {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100000001b3); // FNV prime
        }

        // Spread the hash across 32 bytes
        for i in 0..4 {
            let val = h.wrapping_add(i as u64 * 0x9e3779b97f4a7c15);
            hash[i * 8..(i + 1) * 8].copy_from_slice(&val.to_le_bytes());
        }

        hash
    }

    /// Store a chunk, deduplicating if already present.
    pub fn store_chunk(&mut self, data: &[u8]) -> ChunkHash {
        let hash = self.hash_chunk(data);

        self.stats.total_chunks += 1;
        self.stats.logical_bytes += data.len() as u64;

        if let Some(existing) = self.index.get_mut(&hash) {
            // Already exists — just increment refcount
            existing.ref_count += 1;
            self.stats.dedup_chunks += 1;
        } else {
            // New unique chunk — allocate disk space
            let block_addr = self.next_block;
            self.next_block += ((data.len() + 4095) / 4096) as u64;

            self.index.insert(hash, ChunkRef {
                hash,
                ref_count: 1,
                size: data.len() as u32,
                block_addr,
                compressed: false,
            });

            self.stats.unique_chunks += 1;
            self.stats.physical_bytes += data.len() as u64;
        }

        hash
    }

    /// Store an entire object (auto-chunks + deduplicates).
    pub fn store_object(&mut self, data: &[u8]) -> Vec<ChunkHash> {
        let chunk_boundaries = self.chunk_data(data);
        let mut hashes = Vec::with_capacity(chunk_boundaries.len());

        for (offset, len) in chunk_boundaries {
            let chunk = &data[offset..offset + len];
            let hash = self.store_chunk(chunk);
            hashes.push(hash);
        }

        hashes
    }

    /// Release a chunk reference (for garbage collection).
    pub fn release_chunk(&mut self, hash: &ChunkHash) {
        if let Some(chunk) = self.index.get_mut(hash) {
            chunk.ref_count -= 1;
            if chunk.ref_count == 0 {
                let size = chunk.size as u64;
                self.index.remove(hash);
                self.stats.unique_chunks -= 1;
                self.stats.physical_bytes -= size;
            }
        }
    }

    /// Get dedup statistics.
    pub fn stats(&self) -> &DedupStats {
        &self.stats
    }

    /// Look up a chunk by hash.
    pub fn get_chunk(&self, hash: &ChunkHash) -> Option<&ChunkRef> {
        self.index.get(hash)
    }
}
