//! # Page Cache Silo Quota Bridge (Phase 202)
//!
//! ## Architecture Guardian: The Gap
//! `page_cache.rs` implements `PageCache`:
//! - `set_pool(silo_id, max_pages)` — set per-Silo page cache limit
//! - `lookup(oid, offset, now)` → bool — check if page is cached
//! - `PageCache::new(global_max)` — global limit
//!
//! **Missing link**: `set_pool()` could be called to grant a Silo
//! unlimited cache pages (just pass u64::MAX), starving other Silos.
//!
//! This module provides `PageCacheSiloQuotaBridge`:
//! Caps per-Silo page pool at a configurable maximum (default 4096 pages).

extern crate alloc;

use crate::page_cache::PageCache;

const DEFAULT_MAX_PAGES_PER_SILO: u64 = 4096;

#[derive(Debug, Default, Clone)]
pub struct PageCacheQuotaStats {
    pub pools_set:   u64,
    pub caps_applied: u64,
}

pub struct PageCacheSiloQuotaBridge {
    pub cache:   PageCache,
    max_per_silo: u64,
    pub stats:   PageCacheQuotaStats,
}

impl PageCacheSiloQuotaBridge {
    pub fn new(global_max: u64) -> Self {
        PageCacheSiloQuotaBridge {
            cache: PageCache::new(global_max),
            max_per_silo: DEFAULT_MAX_PAGES_PER_SILO,
            stats: PageCacheQuotaStats::default(),
        }
    }

    /// Set per-Silo page pool with quota cap enforced.
    pub fn set_pool(&mut self, silo_id: u64, requested_pages: u64) {
        self.stats.pools_set += 1;
        let actual = if requested_pages > self.max_per_silo {
            self.stats.caps_applied += 1;
            crate::serial_println!(
                "[PAGE CACHE] Silo {} requested {} pages, capped at {}",
                silo_id, requested_pages, self.max_per_silo
            );
            self.max_per_silo
        } else {
            requested_pages
        };
        self.cache.set_pool(silo_id, actual);
    }

    pub fn lookup(&mut self, oid: u64, offset: u64, now: u64) -> bool {
        self.cache.lookup(oid, offset, now)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PageCacheBridge: pools={} capped={}",
            self.stats.pools_set, self.stats.caps_applied
        );
    }
}
