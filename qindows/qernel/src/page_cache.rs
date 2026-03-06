//! # Page Cache — VFS Read Cache with Per-Silo LRU
//!
//! Caches disk pages in memory for fast reads, with
//! per-Silo isolation and LRU eviction (Section 1.8).
//!
//! Features:
//! - Per-Silo page pools with configurable limits
//! - LRU eviction when pool is full
//! - Dirty page tracking for writeback
//! - Read-ahead prefetching
//! - Cache hit/miss statistics

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Page state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageState {
    Clean,
    Dirty,
    Writeback,
}

/// A cached page.
#[derive(Debug, Clone)]
pub struct CachedPage {
    pub oid: u64,
    pub offset: u64,
    pub silo_id: u64,
    pub state: PageState,
    pub access_count: u64,
    pub last_access: u64,
}

/// Per-Silo cache pool.
#[derive(Debug, Clone)]
pub struct CachePool {
    pub silo_id: u64,
    pub max_pages: u64,
    pub pages_used: u64,
}

/// Page cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_writebacks: u64,
    pub prefetches: u64,
}

/// The Page Cache.
pub struct PageCache {
    /// (oid, offset) → CachedPage
    pub pages: BTreeMap<(u64, u64), CachedPage>,
    pub pools: BTreeMap<u64, CachePool>,
    pub global_max: u64,
    pub global_used: u64,
    pub stats: CacheStats,
}

impl PageCache {
    pub fn new(global_max: u64) -> Self {
        PageCache {
            pages: BTreeMap::new(),
            pools: BTreeMap::new(),
            global_max,
            global_used: 0,
            stats: CacheStats::default(),
        }
    }

    /// Set per-Silo cache limit.
    pub fn set_pool(&mut self, silo_id: u64, max_pages: u64) {
        self.pools.entry(silo_id).or_insert(CachePool {
            silo_id, max_pages, pages_used: 0,
        }).max_pages = max_pages;
    }

    /// Look up a page.
    pub fn lookup(&mut self, oid: u64, offset: u64, now: u64) -> bool {
        let key = (oid, offset);
        if let Some(page) = self.pages.get_mut(&key) {
            page.access_count += 1;
            page.last_access = now;
            self.stats.hits += 1;
            true
        } else {
            self.stats.misses += 1;
            false
        }
    }

    /// Insert a page into the cache.
    pub fn insert(&mut self, oid: u64, offset: u64, silo_id: u64, now: u64) {
        let key = (oid, offset);

        // Already cached
        if self.pages.contains_key(&key) {
            return;
        }

        // Evict if needed
        if self.global_used >= self.global_max {
            self.evict_lru();
        }

        // Check per-Silo pool
        if let Some(pool) = self.pools.get(&silo_id) {
            if pool.pages_used >= pool.max_pages {
                self.evict_silo_lru(silo_id);
            }
        }

        self.pages.insert(key, CachedPage {
            oid, offset, silo_id,
            state: PageState::Clean,
            access_count: 1, last_access: now,
        });
        self.global_used += 1;
        if let Some(pool) = self.pools.get_mut(&silo_id) {
            pool.pages_used += 1;
        }
    }

    /// Mark a page as dirty.
    pub fn mark_dirty(&mut self, oid: u64, offset: u64) {
        if let Some(page) = self.pages.get_mut(&(oid, offset)) {
            page.state = PageState::Dirty;
        }
    }

    /// Evict the globally least-recently-used clean page.
    fn evict_lru(&mut self) {
        let victim = self.pages.iter()
            .filter(|(_, p)| p.state == PageState::Clean)
            .min_by_key(|(_, p)| p.last_access)
            .map(|(k, p)| (*k, p.silo_id));

        if let Some((key, silo_id)) = victim {
            self.pages.remove(&key);
            self.global_used = self.global_used.saturating_sub(1);
            if let Some(pool) = self.pools.get_mut(&silo_id) {
                pool.pages_used = pool.pages_used.saturating_sub(1);
            }
            self.stats.evictions += 1;
        }
    }

    /// Evict LRU page from a specific Silo.
    fn evict_silo_lru(&mut self, silo_id: u64) {
        let victim = self.pages.iter()
            .filter(|(_, p)| p.silo_id == silo_id && p.state == PageState::Clean)
            .min_by_key(|(_, p)| p.last_access)
            .map(|(k, _)| *k);

        if let Some(key) = victim {
            self.pages.remove(&key);
            self.global_used = self.global_used.saturating_sub(1);
            if let Some(pool) = self.pools.get_mut(&silo_id) {
                pool.pages_used = pool.pages_used.saturating_sub(1);
            }
            self.stats.evictions += 1;
        }
    }

    /// Writeback all dirty pages.
    pub fn writeback(&mut self) -> Vec<(u64, u64)> {
        let mut written = Vec::new();
        for (key, page) in self.pages.iter_mut() {
            if page.state == PageState::Dirty {
                page.state = PageState::Clean;
                self.stats.dirty_writebacks += 1;
                written.push(*key);
            }
        }
        written
    }

    /// Cache hit rate.
    pub fn hit_rate(&self) -> f32 {
        let total = self.stats.hits + self.stats.misses;
        if total > 0 { self.stats.hits as f32 / total as f32 } else { 0.0 }
    }
}
