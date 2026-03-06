//! # Q-Thumbnail — Object Preview Generator
//!
//! Generates visual previews and thumbnails for Prism
//! objects to power Aether's live search previews and
//! the Prism Explorer grid view (Section 3.3 / 4.3).
//!
//! Features:
//! - Multi-format thumbnail generation (image, doc, video)
//! - Size-tiered caching (small/medium/large)
//! - Background generation with priority queue
//! - Per-Silo cache isolation
//! - Lazy invalidation on object update

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Thumbnail size tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThumbSize {
    Small,   // 64x64
    Medium,  // 256x256
    Large,   // 512x512
}

impl ThumbSize {
    pub fn pixels(&self) -> u32 {
        match self { ThumbSize::Small => 64, ThumbSize::Medium => 256, ThumbSize::Large => 512 }
    }
}

/// Thumbnail format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbFormat {
    Rgba,
    Webp,
    Sdf, // Vector SDF for Aether rendering
}

/// A cached thumbnail.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub oid: u64,
    pub size: ThumbSize,
    pub format: ThumbFormat,
    pub width: u32,
    pub height: u32,
    pub data_size: u64,
    pub version: u64,
    pub generated_at: u64,
}

/// Generation request priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Immediate,
}

/// A pending generation request.
#[derive(Debug, Clone)]
pub struct GenRequest {
    pub oid: u64,
    pub size: ThumbSize,
    pub priority: Priority,
    pub silo_id: u64,
    pub requested_at: u64,
}

/// Thumbnail cache statistics.
#[derive(Debug, Clone, Default)]
pub struct ThumbStats {
    pub generated: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub invalidated: u64,
    pub bytes_cached: u64,
}

/// The Thumbnail Manager.
pub struct QThumbnail {
    /// Cache: (oid, size) → thumbnail
    pub cache: BTreeMap<(u64, ThumbSize), Thumbnail>,
    /// Pending generation queue (sorted by priority)
    pub queue: Vec<GenRequest>,
    pub max_cache_bytes: u64,
    pub stats: ThumbStats,
}

impl QThumbnail {
    pub fn new(max_cache_bytes: u64) -> Self {
        QThumbnail {
            cache: BTreeMap::new(),
            queue: Vec::new(),
            max_cache_bytes,
            stats: ThumbStats::default(),
        }
    }

    /// Get a cached thumbnail.
    pub fn get(&mut self, oid: u64, size: ThumbSize) -> Option<&Thumbnail> {
        let key = (oid, size);
        if self.cache.contains_key(&key) {
            self.stats.cache_hits += 1;
            self.cache.get(&key)
        } else {
            self.stats.cache_misses += 1;
            None
        }
    }

    /// Request thumbnail generation.
    pub fn request(&mut self, oid: u64, size: ThumbSize, priority: Priority, silo_id: u64, now: u64) {
        // Dedup
        if self.queue.iter().any(|r| r.oid == oid && r.size == size) { return; }
        self.queue.push(GenRequest { oid, size, priority, silo_id, requested_at: now });
        self.queue.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Store a generated thumbnail.
    pub fn store(&mut self, thumb: Thumbnail) {
        self.stats.bytes_cached += thumb.data_size;
        self.stats.generated += 1;
        self.cache.insert((thumb.oid, thumb.size), thumb);
        self.evict_if_needed();
    }

    /// Invalidate thumbnails for an updated object.
    pub fn invalidate(&mut self, oid: u64) {
        let keys: Vec<(u64, ThumbSize)> = self.cache.keys()
            .filter(|(o, _)| *o == oid)
            .copied()
            .collect();
        for key in keys {
            if let Some(t) = self.cache.remove(&key) {
                self.stats.bytes_cached = self.stats.bytes_cached.saturating_sub(t.data_size);
                self.stats.invalidated += 1;
            }
        }
    }

    /// Pop the next generation request.
    pub fn next_request(&mut self) -> Option<GenRequest> {
        if self.queue.is_empty() { None } else { Some(self.queue.remove(0)) }
    }

    /// Evict oldest entries if over budget.
    fn evict_if_needed(&mut self) {
        while self.stats.bytes_cached > self.max_cache_bytes && !self.cache.is_empty() {
            let oldest_key = self.cache.values()
                .min_by_key(|t| t.generated_at)
                .map(|t| (t.oid, t.size));
            if let Some(key) = oldest_key {
                if let Some(t) = self.cache.remove(&key) {
                    self.stats.bytes_cached = self.stats.bytes_cached.saturating_sub(t.data_size);
                }
            } else { break; }
        }
    }
}
