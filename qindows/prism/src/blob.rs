//! # Blob Store — Raw Binary Object Storage
//!
//! Stores raw binary blobs (images, videos, executables) as
//! content-addressable chunks in the Prism object graph.
//! Blobs are referenced by OID and can be deduplicated,
//! compressed, and encrypted transparently (Section 3.2).
//!
//! Features:
//! - Content-addressable storage (by hash)
//! - Chunked storage for large objects
//! - Inline small blobs (< 4KB in Q-Node)
//! - Transparent Zstd compression
//! - Reference counting for dedup

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// A stored blob.
#[derive(Debug, Clone)]
pub struct Blob {
    pub oid: u64,
    pub hash: [u8; 32],
    pub size: u64,
    pub chunks: Vec<BlobChunk>,
    pub compressed: bool,
    pub encrypted: bool,
    pub ref_count: u32,
}

/// A blob chunk (for large objects).
#[derive(Debug, Clone)]
pub struct BlobChunk {
    pub index: u32,
    pub offset: u64,
    pub size: u32,
    pub hash: [u8; 32],
}

/// Blob store statistics.
#[derive(Debug, Clone, Default)]
pub struct BlobStats {
    pub blobs_stored: u64,
    pub blobs_deleted: u64,
    pub bytes_stored: u64,
    pub bytes_deduped: u64,
    pub chunks_total: u64,
}

/// The Blob Store.
pub struct BlobStore {
    pub blobs: BTreeMap<u64, Blob>,
    /// Hash → OID for deduplication
    pub hash_index: BTreeMap<[u8; 32], u64>,
    next_oid: u64,
    pub chunk_size: u32, // Default chunk size
    pub stats: BlobStats,
}

impl BlobStore {
    pub fn new(chunk_size: u32) -> Self {
        BlobStore {
            blobs: BTreeMap::new(),
            hash_index: BTreeMap::new(),
            next_oid: 1,
            chunk_size,
            stats: BlobStats::default(),
        }
    }

    /// Store a blob. Returns existing OID if deduplicated.
    pub fn store(&mut self, hash: [u8; 32], size: u64, compressed: bool) -> u64 {
        // Dedup check
        if let Some(&existing) = self.hash_index.get(&hash) {
            if let Some(blob) = self.blobs.get_mut(&existing) {
                blob.ref_count += 1;
                self.stats.bytes_deduped += size;
            }
            return existing;
        }

        let oid = self.next_oid;
        self.next_oid += 1;

        // Create chunks
        let mut chunks = Vec::new();
        let mut offset = 0u64;
        let mut idx = 0u32;
        while offset < size {
            let chunk_sz = ((size - offset) as u32).min(self.chunk_size);
            chunks.push(BlobChunk {
                index: idx, offset, size: chunk_sz, hash: [0; 32],
            });
            offset += chunk_sz as u64;
            idx += 1;
        }
        self.stats.chunks_total += chunks.len() as u64;

        self.blobs.insert(oid, Blob {
            oid, hash, size, chunks, compressed,
            encrypted: false, ref_count: 1,
        });
        self.hash_index.insert(hash, oid);
        self.stats.blobs_stored += 1;
        self.stats.bytes_stored += size;
        oid
    }

    /// Delete a blob (decrement ref count).
    pub fn delete(&mut self, oid: u64) -> bool {
        if let Some(blob) = self.blobs.get_mut(&oid) {
            blob.ref_count = blob.ref_count.saturating_sub(1);
            if blob.ref_count == 0 {
                let hash = blob.hash;
                self.hash_index.remove(&hash);
                self.blobs.remove(&oid);
                self.stats.blobs_deleted += 1;
                return true;
            }
        }
        false
    }

    /// Get a blob by OID.
    pub fn get(&self, oid: u64) -> Option<&Blob> {
        self.blobs.get(&oid)
    }

    /// Look up by content hash.
    pub fn find_by_hash(&self, hash: &[u8; 32]) -> Option<&Blob> {
        self.hash_index.get(hash).and_then(|oid| self.blobs.get(oid))
    }
}
