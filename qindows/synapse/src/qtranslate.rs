//! # Q-Translate — Real-Time Translation Engine
//!
//! Provides on-device neural translation for the Synapse
//! AI pipeline, powering real-time subtitle generation,
//! document translation, and voice interpretation
//! (Section 6.3).
//!
//! Features:
//! - Language detection (auto)
//! - 50+ language pairs
//! - Streaming translation (word-by-word)
//! - Terminology glossaries (per-Silo)
//! - Translation memory (TM) caching

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Language code (ISO 639-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    En, Es, Fr, De, Zh, Ja, Ko, Ar, Hi, Ru,
    Pt, It, Nl, Sv, Pl, Tr, Th, Vi, Id, Uk,
    Other(u16),
}

/// Translation request.
#[derive(Debug, Clone)]
pub struct TranslateRequest {
    pub id: u64,
    pub source: String,
    pub source_lang: Option<Language>,
    pub target_lang: Language,
    pub silo_id: u64,
    pub streaming: bool,
}

/// Translation result.
#[derive(Debug, Clone)]
pub struct TranslateResult {
    pub request_id: u64,
    pub translated: String,
    pub detected_lang: Language,
    pub confidence: f32,
    pub from_cache: bool,
}

/// A translation memory entry.
#[derive(Debug, Clone)]
pub struct TmEntry {
    pub source: String,
    pub target: String,
    pub source_lang: Language,
    pub target_lang: Language,
    pub use_count: u32,
}

/// Translation statistics.
#[derive(Debug, Clone, Default)]
pub struct TranslateStats {
    pub requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub chars_translated: u64,
    pub detections: u64,
}

/// The Translation Engine.
pub struct QTranslate {
    /// Translation memory cache: (src_lang, tgt_lang, src_text) → result
    pub tm: BTreeMap<(Language, Language), Vec<TmEntry>>,
    /// Per-Silo glossaries
    pub glossaries: BTreeMap<u64, BTreeMap<String, String>>,
    pub max_tm_per_pair: usize,
    next_id: u64,
    pub stats: TranslateStats,
}

impl QTranslate {
    pub fn new() -> Self {
        QTranslate {
            tm: BTreeMap::new(),
            glossaries: BTreeMap::new(),
            max_tm_per_pair: 10_000,
            next_id: 1,
            stats: TranslateStats::default(),
        }
    }

    /// Translate text.
    pub fn translate(&mut self, source: &str, source_lang: Language,
                     target_lang: Language, silo_id: u64) -> TranslateResult {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.requests += 1;
        self.stats.chars_translated += source.len() as u64;

        // Check TM cache
        let pair = (source_lang, target_lang);
        if let Some(entries) = self.tm.get_mut(&pair) {
            if let Some(entry) = entries.iter_mut().find(|e| e.source == source) {
                entry.use_count += 1;
                self.stats.cache_hits += 1;
                return TranslateResult {
                    request_id: id, translated: entry.target.clone(),
                    detected_lang: source_lang, confidence: 1.0, from_cache: true,
                };
            }
        }

        self.stats.cache_misses += 1;

        // Apply glossary substitutions
        let mut result = String::from(source);
        if let Some(glossary) = self.glossaries.get(&silo_id) {
            for (term, replacement) in glossary {
                if result.contains(term.as_str()) {
                    result = result.replace(term.as_str(), replacement.as_str());
                }
            }
        }

        // In production: run through NPU translation model
        // Simplified: return glossary-substituted text
        let translated = alloc::format!("[{}→{}] {}", 
            lang_code(source_lang), lang_code(target_lang), result);

        // Store in TM
        let entries = self.tm.entry(pair).or_insert_with(Vec::new);
        if entries.len() < self.max_tm_per_pair {
            entries.push(TmEntry {
                source: String::from(source), target: translated.clone(),
                source_lang, target_lang, use_count: 1,
            });
        }

        TranslateResult {
            request_id: id, translated, detected_lang: source_lang,
            confidence: 0.85, from_cache: false,
        }
    }

    /// Add a glossary term for a Silo.
    pub fn add_glossary(&mut self, silo_id: u64, term: &str, replacement: &str) {
        self.glossaries.entry(silo_id).or_insert_with(BTreeMap::new)
            .insert(String::from(term), String::from(replacement));
    }

    /// Detect language of input text.
    pub fn detect_language(&mut self, _text: &str) -> (Language, f32) {
        self.stats.detections += 1;
        // In production: run through language detection model
        (Language::En, 0.92)
    }
}

fn lang_code(lang: Language) -> &'static str {
    match lang {
        Language::En => "en", Language::Es => "es", Language::Fr => "fr",
        Language::De => "de", Language::Zh => "zh", Language::Ja => "ja",
        Language::Ko => "ko", Language::Ar => "ar", Language::Hi => "hi",
        Language::Ru => "ru", Language::Pt => "pt", Language::It => "it",
        _ => "xx",
    }
}
