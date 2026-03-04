//! # Synapse Intent Slot Extractor
//!
//! Extracts structured intent slots from natural language queries.
//! Used by the Synapse AI assistant to understand user commands
//! and map them to actionable OS operations.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Recognized intent categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentCategory {
    /// Open an application
    OpenApp,
    /// Search for files/content
    Search,
    /// System settings change
    Settings,
    /// File management (copy, move, delete)
    FileOp,
    /// Network operations
    Network,
    /// Schedule/reminder
    Schedule,
    /// Math/calculation
    Calculate,
    /// Unknown/fallback
    Unknown,
}

/// A slot value extracted from the query.
#[derive(Debug, Clone)]
pub struct Slot {
    /// Slot name (e.g., "app_name", "query", "path")
    pub name: String,
    /// Extracted value
    pub value: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Character span in original text (start, end)
    pub span: (usize, usize),
}

/// A pattern rule for slot extraction.
#[derive(Debug, Clone)]
pub struct SlotPattern {
    /// Trigger keywords
    pub keywords: Vec<String>,
    /// Intent this pattern maps to
    pub intent: IntentCategory,
    /// Slot extractions: (slot_name, position_hint)
    /// position_hint: 0=after keyword, 1=end of query, 2=quoted
    pub slots: Vec<(String, u8)>,
    /// Priority (higher = preferred)
    pub priority: u8,
}

/// Extraction result.
#[derive(Debug, Clone)]
pub struct IntentResult {
    /// Detected intent
    pub intent: IntentCategory,
    /// Extracted slots
    pub slots: Vec<Slot>,
    /// Overall confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Original query
    pub query: String,
}

/// The Intent Slot Extractor.
pub struct SlotExtractor {
    /// Extraction patterns
    pub patterns: Vec<SlotPattern>,
    /// Stats
    pub queries_processed: u64,
    pub intents_matched: u64,
    pub slots_extracted: u64,
}

impl SlotExtractor {
    pub fn new() -> Self {
        let mut ex = SlotExtractor {
            patterns: Vec::new(),
            queries_processed: 0,
            intents_matched: 0,
            slots_extracted: 0,
        };
        ex.load_default_patterns();
        ex
    }

    /// Load built-in patterns.
    fn load_default_patterns(&mut self) {
        // Open app patterns
        self.add_pattern(
            &["open", "launch", "start", "run"],
            IntentCategory::OpenApp,
            &[("app_name", 0)],
            10,
        );

        // Search patterns
        self.add_pattern(
            &["search", "find", "look for", "locate", "where is"],
            IntentCategory::Search,
            &[("query", 0)],
            10,
        );

        // Settings patterns
        self.add_pattern(
            &["set", "change", "adjust", "configure", "toggle"],
            IntentCategory::Settings,
            &[("setting", 0), ("value", 1)],
            8,
        );

        // File operation patterns
        self.add_pattern(
            &["copy", "move", "delete", "rename", "create folder"],
            IntentCategory::FileOp,
            &[("operation", 0), ("path", 1)],
            9,
        );

        // Network patterns
        self.add_pattern(
            &["connect", "disconnect", "ping", "download"],
            IntentCategory::Network,
            &[("action", 0), ("target", 1)],
            7,
        );

        // Schedule patterns
        self.add_pattern(
            &["remind", "schedule", "alarm", "timer", "set timer"],
            IntentCategory::Schedule,
            &[("action", 0), ("time", 1)],
            9,
        );

        // Calculate patterns
        self.add_pattern(
            &["calculate", "what is", "how much", "convert"],
            IntentCategory::Calculate,
            &[("expression", 0)],
            6,
        );
    }

    /// Add an extraction pattern.
    pub fn add_pattern(&mut self, keywords: &[&str], intent: IntentCategory, slots: &[(&str, u8)], priority: u8) {
        self.patterns.push(SlotPattern {
            keywords: keywords.iter().map(|k| String::from(*k)).collect(),
            intent,
            slots: slots.iter().map(|(n, h)| (String::from(*n), *h)).collect(),
            priority,
        });
    }

    /// Extract intent and slots from a natural language query.
    pub fn extract(&mut self, query: &str) -> IntentResult {
        self.queries_processed += 1;
        let lower = query.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();

        let mut best_match: Option<(&SlotPattern, usize, f32)> = None;

        // Find the best matching pattern
        for pattern in &self.patterns {
            for keyword in &pattern.keywords {
                let kw_lower = keyword.to_lowercase();
                if let Some(pos) = lower.find(&kw_lower) {
                    let word_count = kw_lower.split_whitespace().count();
                    // Score: keyword length / query length + priority bonus
                    let score = kw_lower.len() as f32 / lower.len() as f32
                        + pattern.priority as f32 * 0.05;

                    if best_match.as_ref().map(|(_, _, s)| score > *s).unwrap_or(true) {
                        best_match = Some((pattern, pos + kw_lower.len(), score.min(1.0)));
                    }
                    let _ = word_count; // used for multi-word keyword matching
                }
            }
        }

        if let Some((pattern, keyword_end, confidence)) = best_match {
            self.intents_matched += 1;

            let mut slots = Vec::new();
            let remaining = lower[keyword_end..].trim();

            for (slot_name, hint) in &pattern.slots {
                let value = match hint {
                    0 => {
                        // After keyword: take first meaningful word(s)
                        self.extract_after_keyword(remaining)
                    }
                    1 => {
                        // End of query: take last word(s)
                        self.extract_end(remaining)
                    }
                    2 => {
                        // Quoted string
                        self.extract_quoted(query)
                    }
                    _ => String::new(),
                };

                if !value.is_empty() {
                    let span_start = lower.find(&value).unwrap_or(0);
                    slots.push(Slot {
                        name: slot_name.clone(),
                        value: value.clone(),
                        confidence: confidence * 0.9,
                        span: (span_start, span_start + value.len()),
                    });
                    self.slots_extracted += 1;
                }
            }

            IntentResult {
                intent: pattern.intent,
                slots,
                confidence,
                query: String::from(query),
            }
        } else {
            IntentResult {
                intent: IntentCategory::Unknown,
                slots: Vec::new(),
                confidence: 0.0,
                query: String::from(query),
            }
        }
    }

    /// Extract value after the keyword.
    fn extract_after_keyword(&self, remaining: &str) -> String {
        let trimmed = remaining.trim();
        // Skip filler words
        let fillers = ["the", "a", "an", "my", "to", "for", "in"];
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let skip = words.iter()
            .take_while(|w| fillers.contains(w))
            .count();
        let meaningful: Vec<&str> = words[skip..].to_vec();

        if meaningful.is_empty() {
            String::new()
        } else {
            meaningful.join(" ")
        }
    }

    /// Extract value from end of query.
    fn extract_end(&self, remaining: &str) -> String {
        let words: Vec<&str> = remaining.trim().split_whitespace().collect();
        if words.len() > 1 {
            // Take the last segment after "to" or similar
            if let Some(pos) = words.iter().rposition(|w| *w == "to" || *w == "as") {
                return words[pos + 1..].join(" ");
            }
        }
        words.last().map(|w| String::from(*w)).unwrap_or_default()
    }

    /// Extract a quoted string from the query.
    fn extract_quoted(&self, query: &str) -> String {
        if let Some(start) = query.find('"') {
            if let Some(end) = query[start + 1..].find('"') {
                return String::from(&query[start + 1..start + 1 + end]);
            }
        }
        if let Some(start) = query.find('\'') {
            if let Some(end) = query[start + 1..].find('\'') {
                return String::from(&query[start + 1..start + 1 + end]);
            }
        }
        String::new()
    }
}
