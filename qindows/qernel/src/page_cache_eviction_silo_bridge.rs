//! # Page Cache Eviction Silo Bridge (Phase 288)
//!
//! ## Architecture Guardian: The Gap
//! `page_cache.rs` implements `PageCache`:
//! - `CachePool { global_max, pages: Vec<CachedPage> }`
//! - `PageCache::new(global_max: u64)` — create with global limit
//! - `PageState` — Hot, Warm, Cold
//!
//! **Missing link**: Page cache was a global pool with no per-Silo cap.
//! A Silo with a sequential access pattern (cold pages) could fill the
//! entire global cache, triggering eviction of Hot pages from other Silos.
//!
//! This module provides `PageCacheEvictionSiloBridge`:
//! Max 2048 cached pages per Silo.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_CACHE_PAGES_PER_SILO: u64 = 2048;

#[derive(Debug, Default, Clone)]
pub struct PageCacheCapStats {
    pub caches_allowed: u64,
    pub caches_denied:  u64,
}

pub struct PageCacheEvictionSiloBridge {
    silo_page_counts: BTreeMap<u64, u64>,
    pub stats:        PageCacheCapStats,
}

impl PageCacheEvictionSiloBridge {
    pub fn new() -> Self {
        PageCacheEvictionSiloBridge { silo_page_counts: BTreeMap::new(), stats: PageCacheCapStats::default() }
    }

    pub fn allow_cache(&mut self, silo_id: u64) -> bool {
        let count = self.silo_page_counts.entry(silo_id).or_default();
        if *count >= MAX_CACHE_PAGES_PER_SILO {
            self.stats.caches_denied += 1;
            return false;
        }
        *count += 1;
        self.stats.caches_allowed += 1;
        true
    }

    pub fn on_evict(&mut self, silo_id: u64) {
        let count = self.silo_page_counts.entry(silo_id).or_default();
        *count = count.saturating_sub(1);
    }

    pub fn on_vaporize(&mut self, silo_id: u64) {
        self.silo_page_counts.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PageCacheEvictionBridge: allowed={} denied={}",
            self.stats.caches_allowed, self.stats.caches_denied
        );
    }
}
