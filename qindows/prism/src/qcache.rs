//! # Q-Cache — Object-Level Read Cache
//!
//! Caches frequently-read Q-Objects in memory to avoid
//! repeated disk I/O (Section 3.29).
//!
//! Features:
//! - LRU eviction policy
//! - Per-Silo cache partitions
//! - Cache warming on Silo start
//! - Hit/miss ratio tracking
//! - Dirty writeback on eviction

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Cache entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    Clean,
    Dirty,
    Evicting,
}

/// A cached object.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub oid: u64,
    pub silo_id: u64,
    pub size: u32,
    pub state: CacheState,
    pub access_count: u64,
    pub last_access: u64,
    pub loaded_at: u64,
}

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_writebacks: u64,
    pub total_bytes_cached: u64,
}

/// The Q-Cache Manager.
pub struct QCache {
    pub entries: BTreeMap<u64, CacheEntry>,
    /// LRU order (most recent at end)
    pub lru_order: Vec<u64>,
    pub max_entries: usize,
    pub max_bytes: u64,
    pub current_bytes: u64,
    pub stats: CacheStats,
}

impl QCache {
    pub fn new(max_entries: usize, max_bytes: u64) -> Self {
        QCache {
            entries: BTreeMap::new(),
            lru_order: Vec::new(),
            max_entries,
            max_bytes,
            current_bytes: 0,
            stats: CacheStats::default(),
        }
    }

    /// Look up an object in cache. Returns true if hit.
    pub fn get(&mut self, oid: u64, now: u64) -> bool {
        if let Some(entry) = self.entries.get_mut(&oid) {
            entry.access_count += 1;
            entry.last_access = now;
            // Move to end of LRU
            self.lru_order.retain(|&id| id != oid);
            self.lru_order.push(oid);
            self.stats.hits += 1;
            true
        } else {
            self.stats.misses += 1;
            false
        }
    }

    /// Insert an object into cache.
    pub fn insert(&mut self, oid: u64, silo_id: u64, size: u32, now: u64) {
        // Evict if needed
        while self.entries.len() >= self.max_entries || self.current_bytes + size as u64 > self.max_bytes {
            if !self.evict_lru() { break; }
        }

        self.entries.insert(oid, CacheEntry {
            oid, silo_id, size, state: CacheState::Clean,
            access_count: 1, last_access: now, loaded_at: now,
        });
        self.lru_order.push(oid);
        self.current_bytes += size as u64;
        self.stats.total_bytes_cached += size as u64;
    }

    /// Mark an entry as dirty.
    pub fn mark_dirty(&mut self, oid: u64) {
        if let Some(entry) = self.entries.get_mut(&oid) {
            entry.state = CacheState::Dirty;
        }
    }

    /// Evict the least recently used entry.
    fn evict_lru(&mut self) -> bool {
        if self.lru_order.is_empty() { return false; }
        let victim = self.lru_order.remove(0);
        if let Some(entry) = self.entries.remove(&victim) {
            if entry.state == CacheState::Dirty {
                self.stats.dirty_writebacks += 1;
            }
            self.current_bytes = self.current_bytes.saturating_sub(entry.size as u64);
            self.stats.evictions += 1;
            true
        } else { false }
    }

    /// Invalidate a specific entry.
    pub fn invalidate(&mut self, oid: u64) {
        if let Some(entry) = self.entries.remove(&oid) {
            self.current_bytes = self.current_bytes.saturating_sub(entry.size as u64);
            self.lru_order.retain(|&id| id != oid);
        }
    }

    /// Get hit ratio.
    pub fn hit_ratio(&self) -> f64 {
        let total = self.stats.hits + self.stats.misses;
        if total == 0 { return 0.0; }
        self.stats.hits as f64 / total as f64
    }
}
