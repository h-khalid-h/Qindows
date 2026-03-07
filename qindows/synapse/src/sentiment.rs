//! # Synapse Sentiment Analyzer
//!
//! Lexicon-based sentiment analysis for the Synapse AI assistant.
//! Classifies text as positive, negative, or neutral, with
//! intensity scoring and negation handling.

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

/// Sentiment classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentiment {
    VeryPositive,
    Positive,
    Neutral,
    Negative,
    VeryNegative,
}

impl Sentiment {
    pub fn from_score(score: f32) -> Self {
        if score > 0.5 { Sentiment::VeryPositive }
        else if score > 0.1 { Sentiment::Positive }
        else if score > -0.1 { Sentiment::Neutral }
        else if score > -0.5 { Sentiment::Negative }
        else { Sentiment::VeryNegative }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Sentiment::VeryPositive => "Very Positive",
            Sentiment::Positive => "Positive",
            Sentiment::Neutral => "Neutral",
            Sentiment::Negative => "Negative",
            Sentiment::VeryNegative => "Very Negative",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Sentiment::VeryPositive => "😄",
            Sentiment::Positive => "🙂",
            Sentiment::Neutral => "😐",
            Sentiment::Negative => "🙁",
            Sentiment::VeryNegative => "😞",
        }
    }
}

/// Sentiment analysis result.
#[derive(Debug, Clone)]
pub struct SentimentResult {
    /// Overall sentiment
    pub sentiment: Sentiment,
    /// Score (-1.0 to +1.0)
    pub score: f32,
    /// Positive word count
    pub positive_count: u32,
    /// Negative word count
    pub negative_count: u32,
    /// Negation count
    pub negation_count: u32,
    /// Most impactful positive word
    pub top_positive: Option<String>,
    /// Most impactful negative word
    pub top_negative: Option<String>,
}

/// The Sentiment Analyzer.
pub struct SentimentAnalyzer {
    /// Lexicon: word → score
    pub lexicon: BTreeMap<String, f32>,
    /// Negation words
    pub negations: Vec<String>,
    /// Intensifier words (multiply next word's score)
    pub intensifiers: BTreeMap<String, f32>,
    /// Stats
    pub texts_analyzed: u64,
}

impl SentimentAnalyzer {
    pub fn new() -> Self {
        let mut analyzer = SentimentAnalyzer {
            lexicon: BTreeMap::new(),
            negations: Vec::new(),
            intensifiers: BTreeMap::new(),
            texts_analyzed: 0,
        };
        analyzer.load_default_lexicon();
        analyzer
    }

    fn load_default_lexicon(&mut self) {
        // Positive words
        let positives = [
            ("good", 0.5), ("great", 0.7), ("excellent", 0.9), ("amazing", 0.9),
            ("wonderful", 0.8), ("fantastic", 0.9), ("love", 0.8), ("happy", 0.7),
            ("beautiful", 0.7), ("perfect", 0.9), ("awesome", 0.8), ("best", 0.8),
            ("brilliant", 0.8), ("superb", 0.9), ("nice", 0.4), ("fine", 0.3),
            ("pleased", 0.6), ("enjoy", 0.6), ("fun", 0.5), ("helpful", 0.5),
            ("fast", 0.4), ("easy", 0.4), ("smooth", 0.4), ("clean", 0.3),
            ("success", 0.7), ("win", 0.6), ("like", 0.3), ("thank", 0.5),
        ];

        // Negative words
        let negatives = [
            ("bad", -0.5), ("terrible", -0.9), ("horrible", -0.9), ("awful", -0.9),
            ("poor", -0.5), ("hate", -0.8), ("ugly", -0.7), ("worst", -0.9),
            ("broken", -0.6), ("slow", -0.4), ("crash", -0.7), ("error", -0.5),
            ("fail", -0.7), ("bug", -0.5), ("frustrating", -0.7), ("annoying", -0.6),
            ("useless", -0.8), ("stupid", -0.7), ("boring", -0.5), ("waste", -0.6),
            ("problem", -0.4), ("difficult", -0.3), ("confusing", -0.5), ("pain", -0.5),
            ("sad", -0.5), ("angry", -0.6), ("disappointed", -0.6), ("wrong", -0.5),
        ];

        for (word, score) in &positives {
            self.lexicon.insert(String::from(*word), *score);
        }
        for (word, score) in &negatives {
            self.lexicon.insert(String::from(*word), *score);
        }

        // Negation words
        self.negations = ["not", "no", "never", "neither", "nor", "none",
            "cannot", "cant", "dont", "doesnt", "didnt", "wont",
            "wouldnt", "shouldnt", "hardly", "barely", "scarcely"]
            .iter().map(|s| String::from(*s)).collect();

        // Intensifiers
        let intens = [
            ("very", 1.5), ("extremely", 2.0), ("really", 1.3),
            ("absolutely", 2.0), ("incredibly", 1.8), ("totally", 1.5),
            ("completely", 1.5), ("so", 1.3), ("quite", 1.2),
        ];
        for (word, mult) in &intens {
            self.intensifiers.insert(String::from(*word), *mult);
        }
    }

    /// Analyze the sentiment of a text.
    pub fn analyze(&mut self, text: &str) -> SentimentResult {
        self.texts_analyzed += 1;

        let words: Vec<String> = text.to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| !w.is_empty())
            .collect();

        let mut total_score: f32 = 0.0;
        let mut pos_count: u32 = 0;
        let mut neg_count: u32 = 0;
        let mut negation_count: u32 = 0;
        let mut top_pos: Option<(String, f32)> = None;
        let mut top_neg: Option<(String, f32)> = None;
        let mut negate_next = false;
        let mut intensify_next: f32 = 1.0;

        for word in &words {
            // Check negation
            if self.negations.contains(word) {
                negate_next = true;
                negation_count += 1;
                continue;
            }

            // Check intensifier
            if let Some(&mult) = self.intensifiers.get(word) {
                intensify_next = mult;
                continue;
            }

            // Check lexicon
            if let Some(&base_score) = self.lexicon.get(word) {
                let mut score = base_score * intensify_next;
                if negate_next {
                    score = -score * 0.75; // Negation reverses but slightly reduces
                }

                total_score += score;

                if score > 0.0 {
                    pos_count += 1;
                    if top_pos.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                        top_pos = Some((word.clone(), score));
                    }
                } else if score < 0.0 {
                    neg_count += 1;
                    if top_neg.as_ref().map(|(_, s)| score < *s).unwrap_or(true) {
                        top_neg = Some((word.clone(), score));
                    }
                }
            }

            negate_next = false;
            intensify_next = 1.0;
        }

        // Normalize score to -1..1 range
        let word_count = (pos_count + neg_count).max(1) as f32;
        let normalized = (total_score / word_count).max(-1.0).min(1.0);

        SentimentResult {
            sentiment: Sentiment::from_score(normalized),
            score: normalized,
            positive_count: pos_count,
            negative_count: neg_count,
            negation_count,
            top_positive: top_pos.map(|(w, _)| w),
            top_negative: top_neg.map(|(w, _)| w),
        }
    }

    /// Add a word to the lexicon.
    pub fn add_word(&mut self, word: &str, score: f32) {
        self.lexicon.insert(word.to_lowercase(), score.max(-1.0).min(1.0));
    }
}
