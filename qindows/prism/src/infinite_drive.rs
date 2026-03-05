//! # Infinite Drive — Cloud Capability Lazy-Loading
//!
//! Moving files to cloud folders creates Cloud Capabilities (Section 5).
//! Streaming a video lazy-loads only specific Object Chunks.
//! The user sees one seamless namespace — local + cloud unified.
//!
//! Features:
//! - Objects can be **pinned** (always local) or **cloud** (lazy-loaded)
//! - **Chunk-level** granularity: large files load only accessed ranges
//! - **Prefetch**: Predictive loading based on access patterns
//! - **Eviction**: LRU cache management for local storage budget

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Object residency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residency {
    /// Fully local
    Local,
    /// Cloud-only (stub locally, data remote)
    Cloud,
    /// Partially cached (some chunks local)
    Partial,
    /// Currently syncing
    Syncing,
}

/// Pin policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinPolicy {
    /// Always keep local (never evict)
    Pinned,
    /// Keep local when space allows (evictable)
    Cached,
    /// Cloud-only (never cache locally)
    CloudOnly,
}

/// A chunk of an object (64 KiB default).
#[derive(Debug, Clone)]
pub struct ObjectChunk {
    /// Chunk index
    pub index: u32,
    /// Is this chunk locally cached?
    pub local: bool,
    /// Size in bytes
    pub size: u64,
    /// Last access timestamp
    pub last_access: u64,
    /// Access count
    pub access_count: u64,
}

/// A cloud-capable object.
#[derive(Debug, Clone)]
pub struct CloudObject {
    /// Object ID
    pub oid: u64,
    /// Object name
    pub name: String,
    /// Total size (bytes)
    pub total_size: u64,
    /// Chunk size (bytes, default 64 KiB)
    pub chunk_size: u64,
    /// Chunks
    pub chunks: Vec<ObjectChunk>,
    /// Residency
    pub residency: Residency,
    /// Pin policy
    pub pin: PinPolicy,
    /// Cloud provider endpoint hash
    pub cloud_endpoint: u64,
    /// Last access timestamp
    pub last_access: u64,
}

/// Infinite Drive statistics.
#[derive(Debug, Clone, Default)]
pub struct DriveStats {
    pub objects_registered: u64,
    pub chunks_fetched: u64,
    pub chunks_evicted: u64,
    pub bytes_fetched: u64,
    pub bytes_evicted: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub prefetch_hits: u64,
}

/// The Infinite Drive Manager.
pub struct InfiniteDrive {
    /// All objects
    pub objects: BTreeMap<u64, CloudObject>,
    /// Local cache budget (bytes)
    pub cache_budget: u64,
    /// Current cache usage (bytes)
    pub cache_used: u64,
    /// Default chunk size (bytes)
    pub chunk_size: u64,
    /// Statistics
    pub stats: DriveStats,
}

impl InfiniteDrive {
    pub fn new(cache_budget: u64) -> Self {
        InfiniteDrive {
            objects: BTreeMap::new(),
            cache_budget,
            cache_used: 0,
            chunk_size: 64 * 1024, // 64 KiB
            stats: DriveStats::default(),
        }
    }

    /// Register an object.
    pub fn register(&mut self, oid: u64, name: &str, total_size: u64, pin: PinPolicy, cloud_endpoint: u64) {
        let num_chunks = ((total_size + self.chunk_size - 1) / self.chunk_size) as u32;
        let is_local = pin == PinPolicy::Pinned;

        let chunks: Vec<ObjectChunk> = (0..num_chunks).map(|i| {
            let sz = if i as u64 == (num_chunks as u64 - 1) {
                let rem = total_size % self.chunk_size;
                if rem == 0 { self.chunk_size } else { rem }
            } else {
                self.chunk_size
            };
            ObjectChunk { index: i, local: is_local, size: sz, last_access: 0, access_count: 0 }
        }).collect();

        let residency = if is_local { Residency::Local } else { Residency::Cloud };

        if is_local {
            self.cache_used += total_size;
        }

        self.objects.insert(oid, CloudObject {
            oid, name: String::from(name), total_size, chunk_size: self.chunk_size,
            chunks, residency, pin, cloud_endpoint, last_access: 0,
        });
        self.stats.objects_registered += 1;
    }

    /// Access a chunk of an object (fetches from cloud if needed).
    pub fn access_chunk(&mut self, oid: u64, chunk_idx: u32, now: u64) -> Result<bool, &'static str> {
        let obj = self.objects.get_mut(&oid).ok_or("Object not found")?;
        let chunk = obj.chunks.get_mut(chunk_idx as usize).ok_or("Chunk out of range")?;

        chunk.last_access = now;
        chunk.access_count += 1;
        obj.last_access = now;

        if chunk.local {
            self.stats.cache_hits += 1;
            return Ok(true); // Cache hit
        }

        // Cache miss — need to fetch from cloud
        self.stats.cache_misses += 1;

        // Evict if needed
        while self.cache_used + chunk.size > self.cache_budget {
            if !self.evict_lru(now) {
                return Err("Cache full, cannot evict");
            }
        }

        // Fetch chunk
        chunk.local = true;
        self.cache_used += chunk.size;
        self.stats.chunks_fetched += 1;
        self.stats.bytes_fetched += chunk.size;

        // Update residency
        let all_local = obj.chunks.iter().all(|c| c.local);
        obj.residency = if all_local { Residency::Local } else { Residency::Partial };

        Ok(false) // Cache miss (now fetched)
    }

    /// Evict the least-recently-used chunk.
    fn evict_lru(&mut self, _now: u64) -> bool {
        let mut best_oid = 0u64;
        let mut best_chunk = 0u32;
        let mut best_time = u64::MAX;
        let mut best_size = 0u64;

        for obj in self.objects.values() {
            if obj.pin == PinPolicy::Pinned { continue; }
            for chunk in &obj.chunks {
                if chunk.local && chunk.last_access < best_time {
                    best_oid = obj.oid;
                    best_chunk = chunk.index;
                    best_time = chunk.last_access;
                    best_size = chunk.size;
                }
            }
        }

        if best_size == 0 { return false; }

        if let Some(obj) = self.objects.get_mut(&best_oid) {
            if let Some(chunk) = obj.chunks.get_mut(best_chunk as usize) {
                chunk.local = false;
                self.cache_used = self.cache_used.saturating_sub(best_size);
                self.stats.chunks_evicted += 1;
                self.stats.bytes_evicted += best_size;

                let any_local = obj.chunks.iter().any(|c| c.local);
                obj.residency = if any_local { Residency::Partial } else { Residency::Cloud };
                return true;
            }
        }
        false
    }

    /// Prefetch adjacent chunks.
    pub fn prefetch(&mut self, oid: u64, chunk_idx: u32, ahead: u32, now: u64) {
        for i in 1..=ahead {
            let _ = self.access_chunk(oid, chunk_idx + i, now);
        }
    }

    /// Get cache utilization ratio.
    pub fn cache_utilization(&self) -> f32 {
        if self.cache_budget == 0 { return 0.0; }
        self.cache_used as f32 / self.cache_budget as f32
    }
}
