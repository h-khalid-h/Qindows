//! # Input Method Engine — IME for CJK / Multilingual Input
//!
//! Processes keystrokes into complex script characters for
//! languages like Chinese, Japanese, Korean, and others (Section 6.4).
//!
//! Features:
//! - Composition buffer with candidate selection
//! - Multiple input methods (Pinyin, Hiragana, Hangul)
//! - Dictionary lookup with frequency-based ranking
//! - User dictionary (custom words)
//! - Per-Silo IME state isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Input method type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeType {
    Pinyin,
    Hiragana,
    Katakana,
    Hangul,
    Zhuyin,
    Latin,
}

/// IME state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeState {
    Inactive,
    Composing,
    Selecting,
}

/// A candidate word.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub text: String,
    pub reading: String,
    pub frequency: u32,
    pub user_defined: bool,
}

/// IME composition state for one Silo.
#[derive(Debug, Clone)]
pub struct Composition {
    pub silo_id: u64,
    pub ime_type: ImeType,
    pub state: ImeState,
    pub buffer: String,
    pub candidates: Vec<Candidate>,
    pub selected: usize,
}

/// IME statistics.
#[derive(Debug, Clone, Default)]
pub struct ImeStats {
    pub compositions: u64,
    pub commits: u64,
    pub candidates_shown: u64,
    pub user_words_added: u64,
}

/// The Input Method Engine.
pub struct InputMethod {
    /// Per-Silo compositions
    pub compositions: BTreeMap<u64, Composition>,
    /// System dictionary: reading → candidates
    pub dictionary: BTreeMap<String, Vec<Candidate>>,
    /// User dictionary additions
    pub user_dict: BTreeMap<String, Vec<Candidate>>,
    pub default_ime: ImeType,
    pub stats: ImeStats,
}

impl InputMethod {
    pub fn new() -> Self {
        InputMethod {
            compositions: BTreeMap::new(),
            dictionary: BTreeMap::new(),
            user_dict: BTreeMap::new(),
            default_ime: ImeType::Latin,
            stats: ImeStats::default(),
        }
    }

    /// Start or continue composition.
    pub fn compose(&mut self, silo_id: u64, key: char) {
        let comp = self.compositions.entry(silo_id).or_insert_with(|| {
            self.stats.compositions += 1;
            Composition {
                silo_id, ime_type: self.default_ime,
                state: ImeState::Composing,
                buffer: String::new(), candidates: Vec::new(), selected: 0,
            }
        });

        comp.buffer.push(key);
        comp.state = ImeState::Composing;

        // Look up candidates
        let mut candidates = Vec::new();
        if let Some(sys) = self.dictionary.get(&comp.buffer) {
            candidates.extend(sys.iter().cloned());
        }
        if let Some(usr) = self.user_dict.get(&comp.buffer) {
            candidates.extend(usr.iter().cloned());
        }

        // Sort by frequency (highest first)
        candidates.sort_by(|a, b| b.frequency.cmp(&a.frequency));

        if !candidates.is_empty() {
            comp.state = ImeState::Selecting;
            self.stats.candidates_shown += 1;
        }
        comp.candidates = candidates;
        comp.selected = 0;
    }

    /// Select next candidate.
    pub fn next_candidate(&mut self, silo_id: u64) {
        if let Some(comp) = self.compositions.get_mut(&silo_id) {
            if !comp.candidates.is_empty() {
                comp.selected = (comp.selected + 1) % comp.candidates.len();
            }
        }
    }

    /// Commit the selected candidate.
    pub fn commit(&mut self, silo_id: u64) -> Option<String> {
        let comp = self.compositions.get_mut(&silo_id)?;

        let result = if comp.state == ImeState::Selecting && !comp.candidates.is_empty() {
            let text = comp.candidates[comp.selected].text.clone();
            Some(text)
        } else if !comp.buffer.is_empty() {
            Some(comp.buffer.clone())
        } else {
            None
        };

        comp.buffer.clear();
        comp.candidates.clear();
        comp.state = ImeState::Inactive;
        comp.selected = 0;

        if result.is_some() {
            self.stats.commits += 1;
        }
        result
    }

    /// Cancel composition.
    pub fn cancel(&mut self, silo_id: u64) {
        if let Some(comp) = self.compositions.get_mut(&silo_id) {
            comp.buffer.clear();
            comp.candidates.clear();
            comp.state = ImeState::Inactive;
            comp.selected = 0;
        }
    }

    /// Add a word to the user dictionary.
    pub fn add_user_word(&mut self, reading: &str, text: &str, frequency: u32) {
        let candidates = self.user_dict.entry(String::from(reading)).or_insert_with(Vec::new);
        candidates.push(Candidate {
            text: String::from(text), reading: String::from(reading),
            frequency, user_defined: true,
        });
        self.stats.user_words_added += 1;
    }

    /// Set IME type for a Silo.
    pub fn set_ime(&mut self, silo_id: u64, ime_type: ImeType) {
        if let Some(comp) = self.compositions.get_mut(&silo_id) {
            comp.ime_type = ime_type;
        }
    }
}
