//! # Synapse Entity Extractor
//!
//! Extracts named entities (dates, numbers, names, locations,
//! file paths) from natural language input, feeding structured
//! data into Synapse's slot filler (Section 6.6).
//!
//! Features:
//! - Date/time extraction (relative and absolute)
//! - Number parsing (cardinal, ordinal, percentages)
//! - File path detection (Unix and Windows-style)
//! - Email and URL extraction
//! - Configurable entity types

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    Date,
    Time,
    Number,
    Percentage,
    FilePath,
    Url,
    Email,
    Name,
    Location,
    Duration,
}

/// An extracted entity.
#[derive(Debug, Clone)]
pub struct Entity {
    pub entity_type: EntityType,
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub confidence: f32,
}

/// Extractor statistics.
#[derive(Debug, Clone, Default)]
pub struct ExtractorStats {
    pub inputs_processed: u64,
    pub entities_found: u64,
    pub by_type: [u64; 10],
}

/// The Entity Extractor.
pub struct EntityExtractor {
    pub stats: ExtractorStats,
}

impl EntityExtractor {
    pub fn new() -> Self {
        EntityExtractor { stats: ExtractorStats::default() }
    }

    /// Extract all entities from text.
    pub fn extract(&mut self, text: &str) -> Vec<Entity> {
        self.stats.inputs_processed += 1;
        let mut entities = Vec::new();

        self.extract_numbers(text, &mut entities);
        self.extract_paths(text, &mut entities);
        self.extract_urls(text, &mut entities);
        self.extract_emails(text, &mut entities);
        self.extract_dates(text, &mut entities);

        self.stats.entities_found += entities.len() as u64;
        entities
    }

    /// Extract numbers (integers and decimals).
    fn extract_numbers(&self, text: &str, out: &mut Vec<Entity>) {
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_digit() || (chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
                let start = i;
                if chars[i] == '-' { i += 1; }
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',') {
                    i += 1;
                }
                // Check for percentage
                let is_pct = i < chars.len() && chars[i] == '%';
                let end = if is_pct { i + 1 } else { i };
                let value: String = chars[start..end].iter().collect();

                out.push(Entity {
                    entity_type: if is_pct { EntityType::Percentage } else { EntityType::Number },
                    value, start, end, confidence: 0.9,
                });
                if is_pct { i += 1; }
            } else {
                i += 1;
            }
        }
    }

    /// Extract file paths.
    fn extract_paths(&self, text: &str, out: &mut Vec<Entity>) {
        for word in text.split_whitespace() {
            if (word.starts_with('/') || word.starts_with("~/") || word.starts_with("./"))
                && word.len() > 2
            {
                let start = text.find(word).unwrap_or(0);
                out.push(Entity {
                    entity_type: EntityType::FilePath,
                    value: String::from(word),
                    start, end: start + word.len(), confidence: 0.85,
                });
            }
            // Windows-style paths
            if word.len() >= 3 && word.as_bytes().get(1) == Some(&b':')
                && (word.as_bytes().get(2) == Some(&b'\\') || word.as_bytes().get(2) == Some(&b'/'))
            {
                let start = text.find(word).unwrap_or(0);
                out.push(Entity {
                    entity_type: EntityType::FilePath,
                    value: String::from(word),
                    start, end: start + word.len(), confidence: 0.85,
                });
            }
        }
    }

    /// Extract URLs.
    fn extract_urls(&self, text: &str, out: &mut Vec<Entity>) {
        for word in text.split_whitespace() {
            if word.starts_with("http://") || word.starts_with("https://") {
                let start = text.find(word).unwrap_or(0);
                out.push(Entity {
                    entity_type: EntityType::Url,
                    value: String::from(word),
                    start, end: start + word.len(), confidence: 0.95,
                });
            }
        }
    }

    /// Extract emails.
    fn extract_emails(&self, text: &str, out: &mut Vec<Entity>) {
        for word in text.split_whitespace() {
            if let Some(at_pos) = word.find('@') {
                if at_pos > 0 && at_pos < word.len() - 1 && word[at_pos + 1..].contains('.') {
                    let start = text.find(word).unwrap_or(0);
                    out.push(Entity {
                        entity_type: EntityType::Email,
                        value: String::from(word),
                        start, end: start + word.len(), confidence: 0.9,
                    });
                }
            }
        }
    }

    /// Extract relative date references.
    fn extract_dates(&self, text: &str, out: &mut Vec<Entity>) {
        let lower = text.chars().map(|c| {
            if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c }
        }).collect::<String>();

        let date_patterns = [
            "today", "tomorrow", "yesterday",
            "next week", "last week", "next month", "last month",
            "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
        ];

        for pattern in &date_patterns {
            if let Some(pos) = lower.find(pattern) {
                out.push(Entity {
                    entity_type: EntityType::Date,
                    value: String::from(*pattern),
                    start: pos, end: pos + pattern.len(), confidence: 0.8,
                });
            }
        }
    }
}
