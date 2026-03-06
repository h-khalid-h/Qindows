//! # Synapse Intent Classifier
//!
//! Classifies user intents from natural language input to
//! route commands to the appropriate subsystem (Section 6.4).
//!
//! Features:
//! - Multi-label intent classification
//! - Confidence scoring with threshold gating
//! - Per-Silo context memory
//! - Intent history for disambiguation
//! - Fallback to Q-Shell literal parsing

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Recognized intent category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntentType {
    FileOperation,
    SystemQuery,
    AppLaunch,
    WebSearch,
    DeviceControl,
    SettingsChange,
    Communication,
    MediaPlayback,
    Navigation,
    Calculation,
    Translation,
    Unknown,
}

/// A classified intent.
#[derive(Debug, Clone)]
pub struct ClassifiedIntent {
    pub id: u64,
    pub intent_type: IntentType,
    pub confidence: f32,
    pub raw_input: String,
    pub entities: Vec<Entity>,
    pub silo_id: u64,
    pub timestamp: u64,
}

/// An extracted entity (slot).
#[derive(Debug, Clone)]
pub struct Entity {
    pub label: String,
    pub value: String,
    pub start: usize,
    pub end: usize,
}

/// Per-Silo conversation context.
#[derive(Debug, Clone)]
pub struct SiloContext {
    pub silo_id: u64,
    pub recent_intents: Vec<IntentType>,
    pub max_history: usize,
    pub active_topic: Option<IntentType>,
}

/// Intent classifier statistics.
#[derive(Debug, Clone, Default)]
pub struct IntentStats {
    pub classified: u64,
    pub high_confidence: u64,
    pub low_confidence: u64,
    pub fallback_to_shell: u64,
}

/// The Intent Classifier.
pub struct IntentClassifier {
    /// Keyword → intent mapping (simple rule-based layer)
    pub keyword_map: BTreeMap<String, IntentType>,
    /// Per-Silo context
    pub contexts: BTreeMap<u64, SiloContext>,
    /// Confidence threshold for accepting classification
    pub threshold: f32,
    next_id: u64,
    pub stats: IntentStats,
}

impl IntentClassifier {
    pub fn new() -> Self {
        let mut kw = BTreeMap::new();
        // File ops
        for w in &["open", "save", "delete", "copy", "move", "rename", "create"] {
            kw.insert(String::from(*w), IntentType::FileOperation);
        }
        // System
        for w in &["status", "uptime", "memory", "cpu", "disk", "battery"] {
            kw.insert(String::from(*w), IntentType::SystemQuery);
        }
        // Apps
        for w in &["launch", "start", "run", "execute"] {
            kw.insert(String::from(*w), IntentType::AppLaunch);
        }
        // Settings
        for w in &["set", "configure", "change", "toggle", "enable", "disable"] {
            kw.insert(String::from(*w), IntentType::SettingsChange);
        }
        // Media
        for w in &["play", "pause", "stop", "next", "previous", "volume"] {
            kw.insert(String::from(*w), IntentType::MediaPlayback);
        }

        IntentClassifier {
            keyword_map: kw,
            contexts: BTreeMap::new(),
            threshold: 0.6,
            next_id: 1,
            stats: IntentStats::default(),
        }
    }

    /// Classify an input string.
    pub fn classify(&mut self, input: &str, silo_id: u64, now: u64) -> ClassifiedIntent {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.classified += 1;

        let lower = input.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();

        // Score each intent type
        let mut scores: BTreeMap<IntentType, f32> = BTreeMap::new();
        for word in &words {
            let w = String::from(*word);
            if let Some(&intent) = self.keyword_map.get(&w) {
                *scores.entry(intent).or_insert(0.0) += 0.3;
            }
        }

        // Context boost: if recent conversation was about a topic, boost it
        if let Some(ctx) = self.contexts.get(&silo_id) {
            if let Some(topic) = ctx.active_topic {
                *scores.entry(topic).or_insert(0.0) += 0.2;
            }
        }

        // Find best match
        let (intent_type, confidence) = scores.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(core::cmp::Ordering::Equal))
            .map(|(&t, &c)| (t, c.min(1.0)))
            .unwrap_or((IntentType::Unknown, 0.0));

        if confidence >= self.threshold {
            self.stats.high_confidence += 1;
        } else {
            self.stats.low_confidence += 1;
            if intent_type == IntentType::Unknown {
                self.stats.fallback_to_shell += 1;
            }
        }

        // Update context
        let ctx = self.contexts.entry(silo_id).or_insert(SiloContext {
            silo_id, recent_intents: Vec::new(), max_history: 10,
            active_topic: None,
        });
        ctx.recent_intents.push(intent_type);
        if ctx.recent_intents.len() > ctx.max_history {
            ctx.recent_intents.remove(0);
        }
        ctx.active_topic = Some(intent_type);

        ClassifiedIntent {
            id, intent_type, confidence,
            raw_input: String::from(input),
            entities: Vec::new(),
            silo_id, timestamp: now,
        }
    }

    /// Reset context for a Silo.
    pub fn reset_context(&mut self, silo_id: u64) {
        self.contexts.remove(&silo_id);
    }
}

// Helper: lowercase conversion for no_std
trait ToLower {
    fn to_lowercase(&self) -> String;
}

impl ToLower for str {
    fn to_lowercase(&self) -> String {
        self.chars().map(|c| {
            if c.is_ascii_uppercase() {
                (c as u8 + 32) as char
            } else { c }
        }).collect()
    }
}
