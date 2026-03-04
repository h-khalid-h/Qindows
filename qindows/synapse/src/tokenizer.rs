//! # Synapse Tokenizer
//!
//! Text tokenization for the Synapse AI subsystem.
//! Converts raw text into token sequences for embedding,
//! summarization, and intent classification.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A single token.
#[derive(Debug, Clone)]
pub struct Token {
    /// Token text
    pub text: String,
    /// Token type
    pub kind: TokenKind,
    /// Character offset in original text
    pub offset: usize,
    /// Character length
    pub len: usize,
}

/// Token types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A regular word
    Word,
    /// A number (integer or float)
    Number,
    /// Punctuation
    Punctuation,
    /// Whitespace
    Whitespace,
    /// A subword piece (BPE fragment)
    Subword,
    /// Special token ([CLS], [SEP], [PAD], etc.)
    Special,
    /// Unknown / OOV token
    Unknown,
}

/// Tokenization strategy.
#[derive(Debug, Clone, Copy)]
pub enum TokenizerMode {
    /// Split on whitespace and punctuation
    WhitespacePunct,
    /// Byte-Pair Encoding (simplified)
    Bpe,
    /// Character-level
    CharLevel,
    /// Sentence-level (split on sentence boundaries)
    Sentence,
}

/// Tokenizer configuration.
#[derive(Debug, Clone)]
pub struct TokenizerConfig {
    /// Tokenization mode
    pub mode: TokenizerMode,
    /// Convert to lowercase?
    pub lowercase: bool,
    /// Maximum sequence length
    pub max_length: usize,
    /// Add [CLS] at start?
    pub add_cls: bool,
    /// Add [SEP] at end?
    pub add_sep: bool,
    /// Pad to max_length?
    pub pad: bool,
}

impl Default for TokenizerConfig {
    fn default() -> Self {
        TokenizerConfig {
            mode: TokenizerMode::WhitespacePunct,
            lowercase: true,
            max_length: 512,
            add_cls: true,
            add_sep: true,
            pad: false,
        }
    }
}

/// A vocabulary mapping tokens to IDs.
pub struct Vocabulary {
    /// Token → ID
    pub token_to_id: BTreeMap<String, u32>,
    /// ID → Token
    pub id_to_token: BTreeMap<u32, String>,
    /// Next available ID
    next_id: u32,
    /// Special token IDs
    pub pad_id: u32,
    pub unk_id: u32,
    pub cls_id: u32,
    pub sep_id: u32,
}

impl Vocabulary {
    pub fn new() -> Self {
        let mut vocab = Vocabulary {
            token_to_id: BTreeMap::new(),
            id_to_token: BTreeMap::new(),
            next_id: 0,
            pad_id: 0,
            unk_id: 1,
            cls_id: 2,
            sep_id: 3,
        };

        // Reserve special tokens
        vocab.add_token("[PAD]");
        vocab.add_token("[UNK]");
        vocab.add_token("[CLS]");
        vocab.add_token("[SEP]");

        vocab
    }

    /// Add a token to the vocabulary.
    pub fn add_token(&mut self, token: &str) -> u32 {
        if let Some(&id) = self.token_to_id.get(token) {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.token_to_id.insert(String::from(token), id);
        self.id_to_token.insert(id, String::from(token));
        id
    }

    /// Look up a token's ID.
    pub fn get_id(&self, token: &str) -> u32 {
        self.token_to_id.get(token).copied().unwrap_or(self.unk_id)
    }

    /// Look up a token by ID.
    pub fn get_token(&self, id: u32) -> &str {
        self.id_to_token.get(&id).map(|s| s.as_str()).unwrap_or("[UNK]")
    }

    /// Vocabulary size.
    pub fn size(&self) -> u32 {
        self.next_id
    }
}

/// The Tokenizer.
pub struct Tokenizer {
    /// Configuration
    pub config: TokenizerConfig,
    /// Vocabulary
    pub vocab: Vocabulary,
    /// BPE merge rules (pair → merged token)
    pub bpe_merges: Vec<(String, String)>,
    /// Stats
    pub stats: TokenizerStats,
}

/// Tokenizer statistics.
#[derive(Debug, Clone, Default)]
pub struct TokenizerStats {
    pub texts_tokenized: u64,
    pub tokens_produced: u64,
    pub oov_tokens: u64,
    pub truncations: u64,
}

impl Tokenizer {
    pub fn new(config: TokenizerConfig) -> Self {
        Tokenizer {
            config,
            vocab: Vocabulary::new(),
            bpe_merges: Vec::new(),
            stats: TokenizerStats::default(),
        }
    }

    /// Tokenize a text string into tokens.
    pub fn tokenize(&mut self, text: &str) -> Vec<Token> {
        self.stats.texts_tokenized += 1;

        let processed = if self.config.lowercase {
            text.to_lowercase()
        } else {
            String::from(text)
        };

        let mut tokens = match self.config.mode {
            TokenizerMode::WhitespacePunct => self.tokenize_whitespace_punct(&processed),
            TokenizerMode::CharLevel => self.tokenize_char_level(&processed),
            TokenizerMode::Sentence => self.tokenize_sentences(&processed),
            TokenizerMode::Bpe => self.tokenize_bpe(&processed),
        };

        // Add special tokens
        if self.config.add_cls {
            tokens.insert(0, Token {
                text: String::from("[CLS]"),
                kind: TokenKind::Special,
                offset: 0,
                len: 0,
            });
        }
        if self.config.add_sep {
            tokens.push(Token {
                text: String::from("[SEP]"),
                kind: TokenKind::Special,
                offset: text.len(),
                len: 0,
            });
        }

        // Truncate
        if tokens.len() > self.config.max_length {
            tokens.truncate(self.config.max_length);
            self.stats.truncations += 1;
        }

        // Pad
        if self.config.pad {
            while tokens.len() < self.config.max_length {
                tokens.push(Token {
                    text: String::from("[PAD]"),
                    kind: TokenKind::Special,
                    offset: 0,
                    len: 0,
                });
            }
        }

        self.stats.tokens_produced += tokens.len() as u64;
        tokens
    }

    /// Convert tokens to IDs.
    pub fn encode(&mut self, text: &str) -> Vec<u32> {
        let tokens = self.tokenize(text);
        tokens.iter().map(|t| {
            let id = self.vocab.get_id(&t.text);
            if id == self.vocab.unk_id && t.kind != TokenKind::Special {
                self.stats.oov_tokens += 1;
            }
            id
        }).collect()
    }

    /// Decode IDs back to text.
    pub fn decode(&self, ids: &[u32]) -> String {
        ids.iter()
            .map(|&id| self.vocab.get_token(id))
            .filter(|t| !t.starts_with('['))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Build vocabulary from a corpus of text.
    pub fn build_vocab(&mut self, texts: &[&str], min_freq: u32) {
        let mut freq: BTreeMap<String, u32> = BTreeMap::new();

        for text in texts {
            let tokens = self.tokenize_whitespace_punct(text);
            for token in tokens {
                if token.kind == TokenKind::Word || token.kind == TokenKind::Number {
                    *freq.entry(token.text).or_insert(0) += 1;
                }
            }
        }

        for (token, count) in freq {
            if count >= min_freq {
                self.vocab.add_token(&token);
            }
        }
    }

    /// Whitespace + punctuation tokenizer.
    fn tokenize_whitespace_punct(&self, text: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut start = 0;

        for (i, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if !current.is_empty() {
                    let kind = if current.chars().all(|c| c.is_numeric() || c == '.') {
                        TokenKind::Number
                    } else {
                        TokenKind::Word
                    };
                    tokens.push(Token { text: current.clone(), kind, offset: start, len: i - start });
                    current.clear();
                }
                start = i + ch.len_utf8();
            } else if ch.is_ascii_punctuation() {
                if !current.is_empty() {
                    let kind = if current.chars().all(|c| c.is_numeric() || c == '.') {
                        TokenKind::Number
                    } else {
                        TokenKind::Word
                    };
                    tokens.push(Token { text: current.clone(), kind, offset: start, len: i - start });
                    current.clear();
                }
                tokens.push(Token {
                    text: String::from(ch.to_string().as_str()),
                    kind: TokenKind::Punctuation,
                    offset: i,
                    len: ch.len_utf8(),
                });
                start = i + ch.len_utf8();
            } else {
                if current.is_empty() { start = i; }
                current.push(ch);
            }
        }

        if !current.is_empty() {
            let kind = if current.chars().all(|c| c.is_numeric() || c == '.') {
                TokenKind::Number
            } else {
                TokenKind::Word
            };
            tokens.push(Token { text: current, kind, offset: start, len: text.len() - start });
        }

        tokens
    }

    /// Character-level tokenizer.
    fn tokenize_char_level(&self, text: &str) -> Vec<Token> {
        text.char_indices().map(|(i, ch)| {
            let kind = if ch.is_alphabetic() { TokenKind::Word }
            else if ch.is_numeric() { TokenKind::Number }
            else if ch.is_whitespace() { TokenKind::Whitespace }
            else { TokenKind::Punctuation };
            Token {
                text: String::from(ch.to_string().as_str()),
                kind,
                offset: i,
                len: ch.len_utf8(),
            }
        }).collect()
    }

    /// Sentence tokenizer.
    fn tokenize_sentences(&self, text: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut start = 0;

        for (i, ch) in text.char_indices() {
            if ch == '.' || ch == '!' || ch == '?' {
                let end = i + ch.len_utf8();
                let sentence = text[start..end].trim();
                if !sentence.is_empty() {
                    tokens.push(Token {
                        text: String::from(sentence),
                        kind: TokenKind::Word,
                        offset: start,
                        len: end - start,
                    });
                }
                start = end;
            }
        }

        // Last fragment
        let remaining = text[start..].trim();
        if !remaining.is_empty() {
            tokens.push(Token {
                text: String::from(remaining),
                kind: TokenKind::Word,
                offset: start,
                len: text.len() - start,
            });
        }

        tokens
    }

    /// Simplified BPE tokenizer.
    fn tokenize_bpe(&self, text: &str) -> Vec<Token> {
        // Start with whitespace-punct tokens
        let initial = self.tokenize_whitespace_punct(text);
        let mut result = Vec::new();

        for token in initial {
            if token.kind != TokenKind::Word {
                result.push(token);
                continue;
            }

            // Check if in vocab
            if self.vocab.token_to_id.contains_key(&token.text) {
                result.push(token);
                continue;
            }

            // Split into subword pieces
            let chars: Vec<char> = token.text.chars().collect();
            let mut pieces: Vec<String> = chars.iter().map(|c| String::from(*c as char)).collect();

            // Apply BPE merges
            for (a, b) in &self.bpe_merges {
                let merged = alloc::format!("{}{}", a, b);
                let mut i = 0;
                while i + 1 < pieces.len() {
                    if pieces[i] == *a && pieces[i + 1] == *b {
                        pieces[i] = merged.clone();
                        pieces.remove(i + 1);
                    } else {
                        i += 1;
                    }
                }
            }

            for (j, piece) in pieces.into_iter().enumerate() {
                result.push(Token {
                    text: piece,
                    kind: if j == 0 { TokenKind::Word } else { TokenKind::Subword },
                    offset: token.offset,
                    len: token.len,
                });
            }
        }

        result
    }
}
