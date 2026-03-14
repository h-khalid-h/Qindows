//! # Q-Fonts Glyph Cache Rate Bridge (Phase 261)
//!
//! ## Architecture Guardian: The Gap
//! `q_fonts.rs` implements SDF font rendering:
//! - `GlyphSdf { data: &[u8], width, height, silo_id, ... }`
//! - `GlyphMetrics { bbox_left, bbox_right, bbox_top, bbox_bottom }`
//! - `GlyphSdf::sample(u, v)` → f32 — SDF distance sample
//!
//! **Missing link**: Glyph rasterization cache had no per-Silo cap.
//! A Silo could rasterize thousands of unique glyphs at many sizes,
//! filling the cache and evicting other Silos' cached glyphs.
//!
//! This module provides `QFontsGlyphCacheRateBridge`:
//! Max 512 cached glyphs per Silo.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_GLYPH_CACHE_PER_SILO: u64 = 512;

#[derive(Debug, Default, Clone)]
pub struct GlyphCacheStats {
    pub cache_allowed: u64,
    pub cache_denied:  u64,
}

pub struct QFontsGlyphCacheRateBridge {
    silo_glyph_counts: BTreeMap<u64, u64>,
    pub stats:         GlyphCacheStats,
}

impl QFontsGlyphCacheRateBridge {
    pub fn new() -> Self {
        QFontsGlyphCacheRateBridge { silo_glyph_counts: BTreeMap::new(), stats: GlyphCacheStats::default() }
    }

    pub fn allow_cache(&mut self, silo_id: u64) -> bool {
        let count = self.silo_glyph_counts.entry(silo_id).or_default();
        if *count >= MAX_GLYPH_CACHE_PER_SILO {
            self.stats.cache_denied += 1;
            crate::serial_println!(
                "[Q-FONTS] Silo {} glyph cache full ({}/{})", silo_id, count, MAX_GLYPH_CACHE_PER_SILO
            );
            return false;
        }
        *count += 1;
        self.stats.cache_allowed += 1;
        true
    }

    pub fn evict_silo(&mut self, silo_id: u64) {
        self.silo_glyph_counts.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GlyphCacheBridge: allowed={} denied={}",
            self.stats.cache_allowed, self.stats.cache_denied
        );
    }
}
