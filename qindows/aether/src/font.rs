//! # SDF Font Rasterizer
//!
//! Renders text using Signed Distance Field glyph atlases.
//! Each glyph is stored as a small SDF texture (e.g., 32×32 or 48×48),
//! which can be rendered at any size with perfect sharpness.
//! This replaces bitmap fonts and FreeType.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A single glyph's SDF data.
#[derive(Debug, Clone)]
pub struct GlyphSdf {
    /// Unicode codepoint
    pub codepoint: char,
    /// SDF bitmap (signed distance values, 0-255 mapped from -spread to +spread)
    pub sdf_data: Vec<u8>,
    /// Width of SDF texture
    pub width: u16,
    /// Height of SDF texture
    pub height: u16,
    /// Horizontal advance (logical pixels at base size)
    pub advance: f32,
    /// Left side bearing
    pub bearing_x: f32,
    /// Top side bearing
    pub bearing_y: f32,
    /// Base font size this SDF was generated at
    pub base_size: f32,
}

/// Font metrics.
#[derive(Debug, Clone)]
pub struct FontMetrics {
    /// Font ascent (above baseline)
    pub ascent: f32,
    /// Font descent (below baseline, negative)
    pub descent: f32,
    /// Line gap
    pub line_gap: f32,
    /// Units per EM
    pub units_per_em: u16,
    /// Cap height
    pub cap_height: f32,
    /// x-height
    pub x_height: f32,
}

impl FontMetrics {
    /// Line height (ascent - descent + gap).
    pub fn line_height(&self) -> f32 {
        self.ascent - self.descent + self.line_gap
    }
}

/// A font family with SDF glyphs.
#[derive(Debug, Clone)]
pub struct SdfFont {
    /// Font family name
    pub family: String,
    /// Font weight (100-900, 400=regular, 700=bold)
    pub weight: u16,
    /// Is italic?
    pub italic: bool,
    /// Font metrics
    pub metrics: FontMetrics,
    /// Glyph map: codepoint → SDF
    pub glyphs: BTreeMap<char, GlyphSdf>,
    /// SDF spread (distance in pixels that the SDF covers)
    pub sdf_spread: f32,
}

impl SdfFont {
    /// Create a new SDF font with built-in ASCII glyphs.
    pub fn builtin_mono() -> Self {
        let mut font = SdfFont {
            family: String::from("Qindows Mono"),
            weight: 400,
            italic: false,
            metrics: FontMetrics {
                ascent: 12.0,
                descent: -3.0,
                line_gap: 1.0,
                units_per_em: 16,
                cap_height: 10.0,
                x_height: 7.0,
            },
            glyphs: BTreeMap::new(),
            sdf_spread: 4.0,
        };

        // Generate minimal SDF data for printable ASCII
        for cp in 32u8..=126 {
            let ch = cp as char;
            font.glyphs.insert(ch, GlyphSdf {
                codepoint: ch,
                sdf_data: generate_placeholder_sdf(8, 12),
                width: 8,
                height: 12,
                advance: 8.0,
                bearing_x: 0.0,
                bearing_y: 10.0,
                base_size: 16.0,
            });
        }

        font
    }

    /// Get glyph data for a character (with fallback to '?').
    pub fn glyph(&self, ch: char) -> &GlyphSdf {
        self.glyphs.get(&ch)
            .or_else(|| self.glyphs.get(&'?'))
            .unwrap_or_else(|| self.glyphs.values().next().unwrap())
    }

    /// Measure the width of a string at a given font size.
    pub fn measure_width(&self, text: &str, size: f32) -> f32 {
        let scale = size / self.glyphs.values().next()
            .map(|g| g.base_size).unwrap_or(16.0);

        text.chars()
            .map(|ch| self.glyph(ch).advance * scale)
            .sum()
    }

    /// Measure the full bounding box of a string.
    pub fn measure(&self, text: &str, size: f32) -> (f32, f32) {
        let width = self.measure_width(text, size);
        let scale = size / 16.0;
        let height = self.metrics.line_height() * scale;
        (width, height)
    }

    /// Layout a string into positioned glyphs.
    pub fn layout(&self, text: &str, size: f32, x: f32, y: f32) -> Vec<GlyphInstance> {
        let scale = size / self.glyphs.values().next()
            .map(|g| g.base_size).unwrap_or(16.0);

        let mut instances = Vec::with_capacity(text.len());
        let mut cursor_x = x;

        for ch in text.chars() {
            if ch == '\n' {
                // Newline — not handled in single-line layout
                continue;
            }

            let glyph = self.glyph(ch);

            instances.push(GlyphInstance {
                codepoint: ch,
                x: cursor_x + glyph.bearing_x * scale,
                y: y - glyph.bearing_y * scale,
                width: glyph.width as f32 * scale,
                height: glyph.height as f32 * scale,
            });

            cursor_x += glyph.advance * scale;
        }

        instances
    }

    /// Word wrap text to fit within a max width.
    pub fn wrap(&self, text: &str, size: f32, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_width: f32 = 0.0;
        let scale = size / 16.0;

        for word in text.split(' ') {
            let word_width = self.measure_width(word, size);

            if current_width + word_width > max_width && !current_line.is_empty() {
                lines.push(core::mem::take(&mut current_line));
                current_width = 0.0;
            }

            if !current_line.is_empty() {
                current_line.push(' ');
                current_width += self.glyph(' ').advance * scale;
            }

            current_line.push_str(word);
            current_width += word_width;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }
}

/// A positioned glyph ready for rendering.
#[derive(Debug, Clone)]
pub struct GlyphInstance {
    pub codepoint: char,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Generate a placeholder SDF (simple box shape).
fn generate_placeholder_sdf(w: u16, h: u16) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            // Distance to nearest edge (normalized to 0-255)
            let dx = x.min(w - 1 - x) as f32;
            let dy = y.min(h - 1 - y) as f32;
            let dist = dx.min(dy);
            let sdf_val = ((dist / 4.0) * 128.0 + 128.0).min(255.0) as u8;
            data.push(sdf_val);
        }
    }
    data
}

/// The Font Manager — manages loaded fonts.
pub struct FontManager {
    /// Loaded fonts
    pub fonts: Vec<SdfFont>,
    /// Default font index
    pub default_font: usize,
    /// Default font size
    pub default_size: f32,
}

impl FontManager {
    pub fn new() -> Self {
        FontManager {
            fonts: alloc::vec![SdfFont::builtin_mono()],
            default_font: 0,
            default_size: 14.0,
        }
    }

    /// Get the default font.
    pub fn default(&self) -> &SdfFont {
        &self.fonts[self.default_font]
    }

    /// Add a font.
    pub fn add_font(&mut self, font: SdfFont) -> usize {
        self.fonts.push(font);
        self.fonts.len() - 1
    }

    /// Find a font by family name and weight.
    pub fn find(&self, family: &str, weight: u16) -> Option<&SdfFont> {
        self.fonts.iter().find(|f| f.family == family && f.weight == weight)
    }
}
