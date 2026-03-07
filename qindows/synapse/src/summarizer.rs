//! # Synapse Text Summarizer
//!
//! Extractive text summarization for quick document previews.
//! Uses TF-IDF scoring to identify the most important sentences
//! in a document and returns a condensed summary.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::F64Ext;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

/// A scored sentence for ranking.
#[derive(Debug, Clone)]
struct ScoredSentence {
    /// Original sentence text
    text: String,
    /// Position in document (0-indexed)
    position: usize,
    /// TF-IDF importance score
    score: f32,
}

/// Configuration for the summarizer.
#[derive(Debug, Clone)]
pub struct SummaryConfig {
    /// Target number of sentences in summary
    pub target_sentences: usize,
    /// Minimum sentence length (words) to consider
    pub min_sentence_words: usize,
    /// Boost score for first/last sentences
    pub position_boost: f32,
    /// Boost for sentences with numbers / proper nouns
    pub entity_boost: f32,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        SummaryConfig {
            target_sentences: 3,
            min_sentence_words: 5,
            position_boost: 1.5,
            entity_boost: 1.2,
        }
    }
}

/// A generated summary.
#[derive(Debug, Clone)]
pub struct Summary {
    /// Summary text
    pub text: String,
    /// Number of sentences selected
    pub sentence_count: usize,
    /// Compression ratio (summary/original)
    pub compression_ratio: f32,
    /// Top keywords found
    pub keywords: Vec<(String, f32)>,
}

/// Common stop words to exclude from scoring.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "need", "dare", "ought",
    "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "as", "into", "through", "during", "before", "after", "above",
    "below", "between", "out", "off", "over", "under", "again",
    "further", "then", "once", "here", "there", "when", "where",
    "why", "how", "all", "both", "each", "few", "more", "most",
    "other", "some", "such", "no", "not", "only", "own", "same",
    "so", "than", "too", "very", "just", "because", "but", "and",
    "or", "if", "while", "about", "this", "that", "these", "those",
    "it", "its", "i", "me", "my", "we", "our", "you", "your",
    "he", "him", "his", "she", "her", "they", "them", "their",
    "what", "which", "who", "whom",
];

/// Split text into sentences.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if ch == '.' || ch == '!' || ch == '?' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current = String::new();
        }
    }

    // Don't forget the last fragment
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    sentences
}

/// Tokenize a sentence into lowercase words.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Check if a word is a stop word.
fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

/// Compute term frequency for a document.
fn compute_tf(words: &[String]) -> BTreeMap<String, f32> {
    let mut freq = BTreeMap::new();
    let len = words.len() as f32;
    for word in words {
        if !is_stop_word(word) {
            *freq.entry(word.clone()).or_insert(0.0) += 1.0;
        }
    }
    for val in freq.values_mut() {
        *val /= len;
    }
    freq
}

/// Compute inverse document frequency across sentences.
fn compute_idf(sentences_words: &[Vec<String>]) -> BTreeMap<String, f32> {
    let n = sentences_words.len() as f32;
    let mut doc_freq = BTreeMap::new();

    for words in sentences_words {
        let mut seen = alloc::collections::BTreeSet::new();
        for word in words {
            if !is_stop_word(word) && seen.insert(word.clone()) {
                *doc_freq.entry(word.clone()).or_insert(0.0f32) += 1.0;
            }
        }
    }

    let mut idf = BTreeMap::new();
    for (word, df) in doc_freq {
        idf.insert(word, (n / df).ln() + 1.0);
    }
    idf
}

/// Summarize a text document.
pub fn summarize(text: &str, config: &SummaryConfig) -> Summary {
    let sentences = split_sentences(text);

    if sentences.len() <= config.target_sentences {
        return Summary {
            text: text.to_string(),
            sentence_count: sentences.len(),
            compression_ratio: 1.0,
            keywords: Vec::new(),
        };
    }

    // Tokenize all sentences
    let sentences_words: Vec<Vec<String>> = sentences.iter()
        .map(|s| tokenize(s))
        .collect();

    // Compute IDF
    let idf = compute_idf(&sentences_words);

    // Score each sentence
    let mut scored: Vec<ScoredSentence> = sentences.iter()
        .enumerate()
        .zip(sentences_words.iter())
        .map(|((i, text), words)| {
            if words.len() < config.min_sentence_words {
                return ScoredSentence { text: text.clone(), position: i, score: 0.0 };
            }

            // TF-IDF score
            let tf = compute_tf(words);
            let mut tfidf_score: f32 = 0.0;
            for (word, tf_val) in &tf {
                let idf_val = idf.get(word).copied().unwrap_or(1.0);
                tfidf_score += tf_val * idf_val;
            }

            // Position boost (first and last sentences are often important)
            if i == 0 || i == sentences.len() - 1 {
                tfidf_score *= config.position_boost;
            }

            // Entity boost (sentences with numbers or capitalized words)
            let has_entities = words.iter().any(|w| {
                w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    || w.chars().any(|c| c.is_numeric())
            });
            if has_entities { tfidf_score *= config.entity_boost; }

            ScoredSentence { text: text.clone(), position: i, score: tfidf_score }
        })
        .collect();

    // Sort by score (descending)
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(core::cmp::Ordering::Equal));

    // Select top sentences
    let mut selected: Vec<&ScoredSentence> = scored.iter()
        .take(config.target_sentences)
        .collect();

    // Re-sort by original position for natural reading order
    selected.sort_by_key(|s| s.position);

    // Build summary text
    let summary_text: String = selected.iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // Extract top keywords
    let mut all_tf = BTreeMap::new();
    for words in &sentences_words {
        for word in words {
            if !is_stop_word(word) {
                *all_tf.entry(word.clone()).or_insert(0.0f32) += 1.0;
            }
        }
    }
    let mut keywords: Vec<(String, f32)> = all_tf.into_iter()
        .map(|(w, tf)| {
            let idf_val = idf.get(&w).copied().unwrap_or(1.0);
            (w, tf * idf_val)
        })
        .collect();
    keywords.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
    keywords.truncate(10);

    let original_len = text.len() as f32;
    let summary_len = summary_text.len() as f32;

    Summary {
        text: summary_text,
        sentence_count: selected.len(),
        compression_ratio: if original_len > 0.0 { summary_len / original_len } else { 1.0 },
        keywords,
    }
}
