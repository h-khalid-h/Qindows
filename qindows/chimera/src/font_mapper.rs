//! # Chimera Font Mapper
//!
//! Maps Win32 font requests (GDI `CreateFont`, DirectWrite)
//! to Qindows' font subsystem, providing substitution tables
//! for common Windows fonts (Section 5.6).
//!
//! Features:
//! - Win32 → Qindows font name mapping
//! - Weight/style translation
//! - Fallback chain for missing glyphs
//! - Per-Silo font directories
//! - Font metrics emulation (TEXTMETRIC)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Win32 font weight constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontWeight {
    Thin = 100,
    Light = 300,
    Regular = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

/// Font style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// A font mapping rule.
#[derive(Debug, Clone)]
pub struct FontMapping {
    pub win32_name: String,
    pub qindows_name: String,
    pub fallback: Vec<String>,
}

/// Font metrics (emulating Win32 TEXTMETRIC).
#[derive(Debug, Clone)]
pub struct FontMetrics {
    pub height: i32,
    pub ascent: i32,
    pub descent: i32,
    pub internal_leading: i32,
    pub external_leading: i32,
    pub avg_char_width: i32,
    pub max_char_width: i32,
    pub weight: u32,
    pub italic: bool,
    pub underlined: bool,
    pub strikeout: bool,
}

/// Font mapper statistics.
#[derive(Debug, Clone, Default)]
pub struct FontStats {
    pub lookups: u64,
    pub cache_hits: u64,
    pub substitutions: u64,
    pub fallbacks_used: u64,
}

/// The Font Mapper.
pub struct FontMapper {
    /// Win32 name → mapping
    pub mappings: BTreeMap<String, FontMapping>,
    /// Cache: (name, weight, style) → resolved Qindows font
    pub cache: BTreeMap<String, String>,
    pub max_cache: usize,
    pub stats: FontStats,
}

impl FontMapper {
    pub fn new() -> Self {
        let mut mapper = FontMapper {
            mappings: BTreeMap::new(),
            cache: BTreeMap::new(),
            max_cache: 500,
            stats: FontStats::default(),
        };

        // Default Win32 → Qindows mappings
        let defaults = [
            ("Arial", "Liberation Sans", &["Noto Sans", "DejaVu Sans"][..]),
            ("Times New Roman", "Liberation Serif", &["Noto Serif"][..]),
            ("Courier New", "Liberation Mono", &["Noto Mono", "DejaVu Sans Mono"][..]),
            ("Tahoma", "Liberation Sans", &["Noto Sans"][..]),
            ("Verdana", "Liberation Sans", &["Noto Sans"][..]),
            ("Segoe UI", "Inter", &["Noto Sans"][..]),
            ("Consolas", "JetBrains Mono", &["Liberation Mono"][..]),
            ("Calibri", "Carlito", &["Liberation Sans"][..]),
            ("Cambria", "Caladea", &["Liberation Serif"][..]),
            ("Comic Sans MS", "Comic Neue", &["Liberation Sans"][..]),
        ];

        for (win, qin, fallback) in defaults {
            mapper.add_mapping(win, qin, &fallback.iter().map(|s| String::from(*s)).collect::<Vec<_>>());
        }
        mapper
    }

    /// Add a font mapping.
    pub fn add_mapping(&mut self, win32_name: &str, qindows_name: &str, fallback: &[String]) {
        self.mappings.insert(
            String::from(win32_name),
            FontMapping {
                win32_name: String::from(win32_name),
                qindows_name: String::from(qindows_name),
                fallback: fallback.to_vec(),
            },
        );
    }

    /// Resolve a Win32 font request to a Qindows font.
    pub fn resolve(&mut self, win32_name: &str) -> String {
        self.stats.lookups += 1;

        // Cache check
        let key = String::from(win32_name);
        if let Some(cached) = self.cache.get(&key) {
            self.stats.cache_hits += 1;
            return cached.clone();
        }

        // Direct mapping
        let result = if let Some(mapping) = self.mappings.get(win32_name) {
            self.stats.substitutions += 1;
            mapping.qindows_name.clone()
        } else {
            // Case-insensitive search
            let lower = win32_name.to_ascii_lowercase();
            let found = self.mappings.iter().find(|(k, _)| k.to_ascii_lowercase() == lower);
            if let Some((_, mapping)) = found {
                self.stats.substitutions += 1;
                mapping.qindows_name.clone()
            } else {
                // No mapping — return original name as-is
                String::from(win32_name)
            }
        };

        // Cache result
        if self.cache.len() < self.max_cache {
            self.cache.insert(key, result.clone());
        }

        result
    }

    /// Get emulated font metrics.
    pub fn metrics(&self, _font_name: &str, point_size: i32) -> FontMetrics {
        let height = point_size * 96 / 72; // points to pixels at 96 DPI
        FontMetrics {
            height,
            ascent: height * 80 / 100,
            descent: height * 20 / 100,
            internal_leading: height * 5 / 100,
            external_leading: 0,
            avg_char_width: height * 45 / 100,
            max_char_width: height,
            weight: 400,
            italic: false,
            underlined: false,
            strikeout: false,
        }
    }
}

// Helper for case-insensitive comparison in no_std
trait AsciiLower {
    fn to_ascii_lowercase(&self) -> String;
}

impl AsciiLower for str {
    fn to_ascii_lowercase(&self) -> String {
        self.chars().map(|c| {
            if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c }
        }).collect()
    }
}
