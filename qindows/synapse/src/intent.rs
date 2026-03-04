//! # Synapse Intent Pipeline
//!
//! Natural language → system action pipeline.
//! When a user types "open my photos from last week" in Q-Shell,
//! the intent pipeline:
//! 1. Tokenizes the input
//! 2. Classifies the intent (Open, Search, Create, Delete, etc.)
//! 3. Extracts entities (app name, time range, file type)
//! 4. Dispatches to the appropriate subsystem

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Recognized user intents.
#[derive(Debug, Clone)]
pub enum Intent {
    /// Open an app or file
    Open { target: String },
    /// Search for something
    Search { query: String, scope: SearchScope },
    /// Create a new item
    Create { item_type: String, name: Option<String> },
    /// Delete/remove an item
    Delete { target: String },
    /// Change a setting
    SetSetting { key: String, value: String },
    /// Navigate to a location
    Navigate { destination: String },
    /// Run a system command
    SystemCommand { command: String },
    /// Ask a question
    Query { question: String },
    /// Undo the last action
    Undo,
    /// Unknown intent (fall back to literal)
    Unknown { raw: String },
}

/// Search scope.
#[derive(Debug, Clone)]
pub enum SearchScope {
    /// Search everywhere
    All,
    /// Search in files
    Files,
    /// Search in apps
    Apps,
    /// Search in settings
    Settings,
    /// Search in web
    Web,
    /// Search with time constraint
    TimeBound { after: Option<String>, before: Option<String> },
}

/// An extracted entity from the input.
#[derive(Debug, Clone)]
pub struct Entity {
    /// Entity type (e.g., "app_name", "file_type", "time_range")
    pub entity_type: String,
    /// Entity value
    pub value: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Character span in original input
    pub start: usize,
    pub end: usize,
}

/// Classification result.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// The classified intent
    pub intent: Intent,
    /// Extracted entities
    pub entities: Vec<Entity>,
    /// Overall confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Alternative interpretations
    pub alternatives: Vec<(Intent, f32)>,
}

/// Intent keywords for rule-based classification.
struct IntentKeywords {
    open: Vec<&'static str>,
    search: Vec<&'static str>,
    create: Vec<&'static str>,
    delete: Vec<&'static str>,
    setting: Vec<&'static str>,
    navigate: Vec<&'static str>,
    undo: Vec<&'static str>,
}

static KEYWORDS: IntentKeywords = IntentKeywords {
    open: alloc::vec!["open", "launch", "start", "run", "execute"],
    search: alloc::vec!["find", "search", "look", "where", "locate", "show"],
    create: alloc::vec!["create", "new", "make", "add", "write"],
    delete: alloc::vec!["delete", "remove", "trash", "erase", "clear"],
    setting: alloc::vec!["set", "change", "toggle", "enable", "disable", "turn"],
    navigate: alloc::vec!["go", "navigate", "switch", "move"],
    undo: alloc::vec!["undo", "revert", "rollback"],
};

/// The Intent Pipeline.
pub struct IntentPipeline {
    /// Enable neural classification (requires Synapse models)
    pub neural_enabled: bool,
    /// Minimum confidence for auto-dispatch
    pub min_confidence: f32,
    /// Total classifications performed
    pub total_classified: u64,
}

impl IntentPipeline {
    pub fn new() -> Self {
        IntentPipeline {
            neural_enabled: false,
            min_confidence: 0.6,
            total_classified: 0,
        }
    }

    /// Classify a natural language input.
    pub fn classify(&mut self, input: &str) -> ClassificationResult {
        self.total_classified += 1;

        let tokens: Vec<&str> = input.split_whitespace().collect();
        let lower = input.to_lowercase();

        // Rule-based intent classification
        let (intent, confidence) = self.rule_classify(&lower, &tokens);

        // Extract entities
        let entities = self.extract_entities(&lower, &tokens);

        ClassificationResult {
            intent,
            entities,
            confidence,
            alternatives: Vec::new(),
        }
    }

    /// Rule-based classification using keyword matching.
    fn rule_classify(&self, input: &str, tokens: &[&str]) -> (Intent, f32) {
        if tokens.is_empty() {
            return (Intent::Unknown { raw: String::from(input) }, 0.0);
        }

        let first = tokens[0].to_lowercase();
        let rest = if tokens.len() > 1 {
            tokens[1..].join(" ")
        } else {
            String::new()
        };

        // Check each intent category
        if ["open", "launch", "start", "run", "execute"].contains(&first.as_str()) {
            return (Intent::Open { target: rest }, 0.9);
        }

        if ["find", "search", "locate", "where"].contains(&first.as_str()) {
            let scope = if input.contains("file") {
                SearchScope::Files
            } else if input.contains("app") {
                SearchScope::Apps
            } else if input.contains("setting") {
                SearchScope::Settings
            } else {
                SearchScope::All
            };
            return (Intent::Search { query: rest, scope }, 0.85);
        }

        if ["create", "new", "make", "add"].contains(&first.as_str()) {
            return (Intent::Create { item_type: rest.clone(), name: Some(rest) }, 0.85);
        }

        if ["delete", "remove", "trash", "erase"].contains(&first.as_str()) {
            return (Intent::Delete { target: rest }, 0.85);
        }

        if ["set", "change", "toggle", "enable", "disable"].contains(&first.as_str()) {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let key = String::from(parts.first().copied().unwrap_or(""));
            let value = String::from(parts.get(1).copied().unwrap_or(""));
            return (Intent::SetSetting { key, value }, 0.8);
        }

        if ["go", "navigate", "switch"].contains(&first.as_str()) {
            return (Intent::Navigate { destination: rest }, 0.8);
        }

        if ["undo", "revert", "rollback"].contains(&first.as_str()) {
            return (Intent::Undo, 0.95);
        }

        // Show-based queries
        if first == "show" {
            return (Intent::Search {
                query: rest,
                scope: SearchScope::All,
            }, 0.75);
        }

        // Fall back to system command
        (Intent::SystemCommand { command: String::from(input) }, 0.5)
    }

    /// Extract named entities from the input.
    fn extract_entities(&self, input: &str, _tokens: &[&str]) -> Vec<Entity> {
        let mut entities = Vec::new();

        // Time expressions
        let time_words = [
            "today", "yesterday", "last week", "last month",
            "this week", "this month", "recent",
        ];
        for tw in &time_words {
            if let Some(pos) = input.find(tw) {
                entities.push(Entity {
                    entity_type: String::from("time_range"),
                    value: String::from(*tw),
                    confidence: 0.9,
                    start: pos,
                    end: pos + tw.len(),
                });
            }
        }

        // File type expressions
        let file_types = [
            ("photo", "image"), ("picture", "image"), ("image", "image"),
            ("video", "video"), ("movie", "video"),
            ("document", "document"), ("file", "file"),
            ("music", "audio"), ("song", "audio"),
        ];
        for (word, ftype) in &file_types {
            if input.contains(word) {
                entities.push(Entity {
                    entity_type: String::from("file_type"),
                    value: String::from(*ftype),
                    confidence: 0.85,
                    start: input.find(word).unwrap_or(0),
                    end: input.find(word).unwrap_or(0) + word.len(),
                });
            }
        }

        entities
    }

    /// Dispatch a classified intent to the appropriate subsystem.
    pub fn dispatch(&self, result: &ClassificationResult) -> String {
        if result.confidence < self.min_confidence {
            return alloc::format!(
                "I'm not sure what you mean (confidence: {:.0}%). Could you rephrase?",
                result.confidence * 100.0
            );
        }

        match &result.intent {
            Intent::Open { target } =>
                alloc::format!("Opening: {}", target),
            Intent::Search { query, .. } =>
                alloc::format!("Searching for: {}", query),
            Intent::Create { item_type, .. } =>
                alloc::format!("Creating: {}", item_type),
            Intent::Delete { target } =>
                alloc::format!("Deleting: {}", target),
            Intent::SetSetting { key, value } =>
                alloc::format!("Setting {} = {}", key, value),
            Intent::Navigate { destination } =>
                alloc::format!("Navigating to: {}", destination),
            Intent::SystemCommand { command } =>
                alloc::format!("Running: {}", command),
            Intent::Query { question } =>
                alloc::format!("Answering: {}", question),
            Intent::Undo =>
                String::from("Undoing last action..."),
            Intent::Unknown { raw } =>
                alloc::format!("Unknown command: {}", raw),
        }
    }
}
