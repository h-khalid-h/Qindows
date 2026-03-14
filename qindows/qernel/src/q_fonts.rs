//! # Q-Fonts — Vector Font Rasterization Engine (Phase 95)
//!
//! ARCHITECTURE.md §4 — Aether: Vector-Native UI:
//! > "All text is vector-native — fonts rendered via SDF glyph atlases"
//! > "Infinite zoom without pixelation"
//! > "Font rendering is part of the SDF scene graph"
//!
//! ## Architecture Guardian: What was missing
//! `aether.rs` renders SDF scenes. But text rendering in Aether requires:
//! 1. A font file loaded from Prism (TrueType/OpenType)
//! 2. Each glyph converted to an SDF representation
//! 3. The SDF glyph "stamped" into the scene graph at the right position
//!
//! **This module** provides:
//! - `GlyphSdf`: a glyph's bounding box + SDF grid (signed distance field, 64×64)
//! - `FontMetrics`: advance widths, kerning pairs, ascent/descent
//! - `TextLayout`: positions a string of glyphs for a given font size and width
//! - `SdfTextPrimitive`: what Aether's compositor receives (a Vec of positioned SDFs)
//!
//! ## Why SDF for fonts?
//! ```text
//! Traditional bitmap:  Scale 2× → blurry pixels
//! SDF glyph:           Scale any amount → mathematically perfect edges
//!   distance(point, glyph_outline) > 0  → outside
//!   distance(point, glyph_outline) < 0  → inside
//!   distance = 0                         → exactly on the edge
//! Aether compositor: threshold SDF at 0 → crisp glyph at any size
//! ```
//!
//! ## Performance
//! - Glyph SDFs cached per (font_oid, codepoint) — computed once, reused forever
//! - 64×64 SDF grid = 4096 bytes per glyph (fits entirely in CPU L2 cache)
//! - At 120 FPS, 1000 glyphs visible: 4MB working set — well within cache budget
//!
//! ## Law 4 Compliance
//! "Vector-native UI" is strictly maintained:
//! - No bitmap glyphs ever (SDF always)
//! - Characters addressable at sub-pixel precision

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── SDF Grid Resolution ───────────────────────────────────────────────────────

/// SDF glyph grid size (64×64 = 4096 i8 values).
pub const SDF_GRID: usize = 64;

/// A precomputed SDF grid for one glyph.
/// Values: negative = inside glyph, positive = outside, 0 = on edge.
/// Range: -127 to +127 (mapped to actual distances during rendering).
pub type SdfGrid = [[i8; SDF_GRID]; SDF_GRID];

// ── Glyph Metrics ─────────────────────────────────────────────────────────────

/// Measured properties of one glyph.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlyphMetrics {
    /// Horizontal advance (how far to move cursor after this glyph), in font units
    pub advance_width: u16,
    /// Left bearing (distance from cursor to left edge of glyph bounding box)
    pub left_bearing: i16,
    /// Glyph bounding box (in font units, relative to origin)
    pub bbox_left: i16,
    pub bbox_right: i16,
    pub bbox_top: i16,
    pub bbox_bottom: i16,
}

impl GlyphMetrics {
    pub fn width(&self) -> i16 { self.bbox_right - self.bbox_left }
    pub fn height(&self) -> i16 { self.bbox_top - self.bbox_bottom }
}

// ── SDF Glyph ─────────────────────────────────────────────────────────────────

/// An SDF glyph ready for use by the Aether compositor.
#[derive(Debug, Clone)]
pub struct GlyphSdf {
    pub codepoint: u32,
    pub metrics: GlyphMetrics,
    /// The SDF grid (64×64 signed distance values)
    pub grid: SdfGrid,
    /// True if this is a whitespace/invisible glyph (no grid needed)
    pub whitespace: bool,
}

impl GlyphSdf {
    /// Sample the SDF at a normalised position (0.0-1.0 within glyph bounds).
    /// Returns signed distance; threshold at 0 to determine inside/outside.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        if self.whitespace { return 1.0; } // outside
        let x = (u * (SDF_GRID as f32)) as usize;
        let y = (v * (SDF_GRID as f32)) as usize;
        let x = x.min(SDF_GRID - 1);
        let y = y.min(SDF_GRID - 1);
        self.grid[y][x] as f32 / 127.0 // normalise to -1.0..+1.0
    }
}

// ── Font Metrics ──────────────────────────────────────────────────────────────

/// Global metrics for a font face.
#[derive(Debug, Clone, Default)]
pub struct FontMetrics {
    pub font_oid: [u8; 32],
    pub family_name: String,
    pub is_bold: bool,
    pub is_italic: bool,
    /// Font units per EM square (typically 1000 or 2048)
    pub units_per_em: u16,
    /// Ascender height above baseline (font units)
    pub ascender: i16,
    /// Descender depth below baseline (negative, font units)
    pub descender: i16,
    /// Line gap between lines
    pub line_gap: i16,
    /// Number of glyphs in this font
    pub glyph_count: u16,
}

impl FontMetrics {
    pub fn line_height_px(&self, size_px: f32) -> f32 {
        let scale = size_px / self.units_per_em as f32;
        (self.ascender - self.descender + self.line_gap) as f32 * scale
    }
    pub fn ascender_px(&self, size_px: f32) -> f32 {
        (self.ascender as f32 / self.units_per_em as f32) * size_px
    }
}

// ── Kerning Pair ──────────────────────────────────────────────────────────────

/// Kerning adjustment between two adjacent glyphs.
#[derive(Debug, Clone, Copy)]
pub struct KerningPair {
    pub left: u32,   // left codepoint
    pub right: u32,  // right codepoint
    pub adjustment: i16, // in font units (typically negative = move closer)
}

// ── Font Resource ─────────────────────────────────────────────────────────────

/// A loaded font with SDF glyph cache.
pub struct FontResource {
    pub metrics: FontMetrics,
    /// SDF glyph cache: codepoint → GlyphSdf
    pub glyph_cache: BTreeMap<u32, GlyphSdf>,
    /// Kerning pairs: (left_cp, right_cp) key → adjustment
    pub kerning: BTreeMap<u64, i16>,
}

impl FontResource {
    pub fn new(metrics: FontMetrics) -> Self {
        FontResource {
            metrics,
            glyph_cache: BTreeMap::new(),
            kerning: BTreeMap::new(),
        }
    }

    fn kern_key(left: u32, right: u32) -> u64 {
        ((left as u64) << 32) | right as u64
    }

    pub fn add_kerning(&mut self, pair: KerningPair) {
        self.kerning.insert(Self::kern_key(pair.left, pair.right), pair.adjustment);
    }

    pub fn kerning_for(&self, left: u32, right: u32) -> i16 {
        *self.kerning.get(&Self::kern_key(left, right)).unwrap_or(&0)
    }

    /// Register a precomputed glyph SDF (typically called by font loader).
    pub fn register_glyph(&mut self, glyph: GlyphSdf) {
        self.glyph_cache.insert(glyph.codepoint, glyph);
    }

    /// Get a glyph, returning a fallback rectangle SDF if not found.
    pub fn get_glyph(&self, codepoint: u32) -> Option<&GlyphSdf> {
        self.glyph_cache.get(&codepoint)
    }

    /// Generate a simple synthetic SDF for a basic codepoint (for bootstrapping).
    /// In production this would be computed from actual font outline data.
    pub fn synthesize_glyph(&mut self, codepoint: u32) -> &GlyphSdf {
        if !self.glyph_cache.contains_key(&codepoint) {
            // Synthesise a glyph SDF: filled rectangle with rounded corners
            let mut grid = [[0i8; SDF_GRID]; SDF_GRID];
            let half = SDF_GRID as i32 / 2;
            let radius = 20i32;
            for y in 0..SDF_GRID {
                for x in 0..SDF_GRID {
                    let dx = x as i32 - half;
                    let dy = y as i32 - half;
                    // SDF of a rounded box: inside = negative
                    let qx = dx.abs() - radius;
                    let qy = dy.abs() - radius;
                    let dist = if qx > 0 && qy > 0 {
                        // corner region
                        // Integer sqrt approximation (no stdlib): Newton's method, 3 iterations
                        let sq = (qx * qx + qy * qy) as u64;
                        let mut d = sq as i32;
                        if sq > 0 {
                            let mut est = sq as i32;
                            est = (est + 1) / 2;
                            est = (est + sq as i32 / est.max(1)) / 2;
                            est = (est + sq as i32 / est.max(1)) / 2;
                            d = est;
                        }
                        d.min(127)
                    } else {
                        qx.max(qy).min(127)
                    };
                    grid[y][x] = dist.min(127).max(-127) as i8;
                }
            }

            self.glyph_cache.insert(codepoint, GlyphSdf {
                codepoint,
                metrics: GlyphMetrics {
                    advance_width: 600,
                    left_bearing: 50,
                    bbox_left: 0, bbox_right: 600, bbox_top: 700, bbox_bottom: 0,
                },
                grid,
                whitespace: false,
            });
        }
        self.glyph_cache.get(&codepoint).unwrap()
    }
}

// ── Text Layout ───────────────────────────────────────────────────────────────

/// A positioned glyph in a text layout result.
#[derive(Debug, Clone)]
pub struct PositionedGlyph {
    pub codepoint: u32,
    /// Screen position (top-left of glyph bounding box, pixels)
    pub x: f32, pub y: f32,
    /// Pixel dimensions
    pub w: f32, pub h: f32,
    /// SDF scale factor (size_px / units_per_em)
    pub scale: f32,
}

/// Result of laying out a string of text.
#[derive(Debug, Clone, Default)]
pub struct TextLayout {
    pub glyphs: Vec<PositionedGlyph>,
    /// Total advance width (pixels)
    pub total_width: f32,
    /// Total height (pixels, includes ascender + descender)
    pub total_height: f32,
    /// Number of lines
    pub lines: u32,
}

// ── Q-Fonts Engine ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct FontEngineStats {
    pub fonts_loaded: u64,
    pub glyphs_cached: u64,
    pub layouts_computed: u64,
    pub glyphs_rendered: u64,
}

/// Vector font SDF rasterization engine.
pub struct QFontEngine {
    /// Loaded fonts: font_oid_key → FontResource
    pub fonts: BTreeMap<u64, FontResource>,
    /// Active font OID key (default font for Q-Shell / Aether)
    pub default_font: Option<u64>,
    pub stats: FontEngineStats,
}

impl QFontEngine {
    pub fn new() -> Self {
        QFontEngine {
            fonts: BTreeMap::new(),
            default_font: None,
            stats: FontEngineStats::default(),
        }
    }

    fn oid_key(oid: &[u8; 32]) -> u64 {
        u64::from_le_bytes([oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7]])
    }

    /// Register a font resource (called by font loader from Prism).
    pub fn register_font(&mut self, resource: FontResource) {
        let key = Self::oid_key(&resource.metrics.font_oid);
        crate::serial_println!(
            "[QFONTS] Registered '{}' ({} glyphs, {}UPM) OID={:02x}{:02x}..",
            resource.metrics.family_name,
            resource.glyph_cache.len(),
            resource.metrics.units_per_em,
            resource.metrics.font_oid[0], resource.metrics.font_oid[1]
        );
        if self.default_font.is_none() { self.default_font = Some(key); }
        self.fonts.insert(key, resource);
        self.stats.fonts_loaded += 1;
    }

    /// Layout a string using a specific font at a given pixel size.
    pub fn layout_text(
        &mut self,
        text: &str,
        font_oid: Option<[u8; 32]>,
        size_px: f32,
        max_width: Option<f32>,
    ) -> TextLayout {
        let font_key = font_oid
            .map(|o| Self::oid_key(&o))
            .or(self.default_font);

        let font_key = match font_key {
            Some(k) => k,
            None => return TextLayout::default(),
        };

        // Synthesize if font missing
        if !self.fonts.contains_key(&font_key) { return TextLayout::default(); }

        let units_per_em = self.fonts[&font_key].metrics.units_per_em as f32;
        let scale = size_px / units_per_em;
        let ascender = self.fonts[&font_key].metrics.ascender_px(size_px);
        let line_h = self.fonts[&font_key].metrics.line_height_px(size_px);

        let mut layout = TextLayout { lines: 1, ..Default::default() };
        let mut cursor_x = 0.0f32;
        let mut cursor_y = 0.0f32;

        let codepoints: Vec<u32> = text.chars().map(|c| c as u32).collect();

        for (i, &cp) in codepoints.iter().enumerate() {
            // Handle newlines
            if cp == '\n' as u32 {
                cursor_x = 0.0;
                cursor_y += line_h;
                layout.lines += 1;
                continue;
            }

            // Synthesize glyph if not cached
            {
                let font = self.fonts.get_mut(&font_key).unwrap();
                if !font.glyph_cache.contains_key(&cp) {
                    font.synthesize_glyph(cp);
                    self.stats.glyphs_cached += 1;
                }
            }

            let font = self.fonts.get(&font_key).unwrap();
            let glyph = font.get_glyph(cp);

            if let Some(g) = glyph {
                if g.whitespace {
                    cursor_x += g.metrics.advance_width as f32 * scale;
                    continue;
                }

                let gw = (g.metrics.bbox_right - g.metrics.bbox_left) as f32 * scale;
                let gh = (g.metrics.bbox_top - g.metrics.bbox_bottom) as f32 * scale;
                let gx = cursor_x + g.metrics.left_bearing as f32 * scale;
                let gy = cursor_y + ascender - gh;

                // Line wrap
                if let Some(max_w) = max_width {
                    if cursor_x + gw > max_w {
                        cursor_x = 0.0;
                        cursor_y += line_h;
                        layout.lines += 1;
                    }
                }

                layout.glyphs.push(PositionedGlyph {
                    codepoint: cp,
                    x: gx, y: gy, w: gw, h: gh, scale,
                });

                // Apply kerning
                let kern = if i + 1 < codepoints.len() {
                    font.kerning_for(cp, codepoints[i + 1]) as f32 * scale
                } else { 0.0 };

                cursor_x += g.metrics.advance_width as f32 * scale + kern;
            }
        }

        layout.total_width = cursor_x;
        layout.total_height = cursor_y + line_h;
        self.stats.layouts_computed += 1;
        self.stats.glyphs_rendered += layout.glyphs.len() as u64;

        layout
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-Fonts SDF Engine (§4)            ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Fonts loaded:  {:>6}                ║", self.stats.fonts_loaded);
        crate::serial_println!("║ Glyphs cached: {:>6}                ║", self.stats.glyphs_cached);
        crate::serial_println!("║ Layouts done:  {:>6}                ║", self.stats.layouts_computed);
        crate::serial_println!("║ Glyphs rendered:{:>6}K              ║", self.stats.glyphs_rendered / 1000);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
