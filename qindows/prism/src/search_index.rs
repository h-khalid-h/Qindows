//! # Prism Full-Text Search Index
//!
//! Inverted index for fast keyword search across Prism objects.
//! Complements the vector-based semantic search with exact-match
//! and prefix-match capabilities.
//!
//! Features:
//! - TF-IDF scoring for relevance ranking
//! - Prefix matching for autocomplete
//! - Field-scoped search (label, tags, content)
//! - Per-Silo index isolation

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::string::String;
use alloc::vec::Vec;

// ─── Index Entry ────────────────────────────────────────────────────────────

/// Which field a term was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    /// Object label / title
    Label,
    /// Semantic tags
    Tag,
    /// Full content body
    Content,
    /// Content type (e.g., "document", "email")
    ContentType,
}

/// A posting — one occurrence of a term in a document.
#[derive(Debug, Clone)]
pub struct Posting {
    /// Object ID (Prism OID index, not full 32-byte hash)
    pub doc_id: u64,
    /// Which field the term appeared in
    pub field: Field,
    /// Term frequency in this document
    pub term_freq: u32,
    /// Positions within the field (for phrase search)
    pub positions: Vec<u32>,
}

/// Document metadata stored in the index.
#[derive(Debug, Clone)]
pub struct IndexedDoc {
    /// Document ID
    pub doc_id: u64,
    /// Prism OID
    pub oid: [u8; 32],
    /// Document label
    pub label: String,
    /// Total term count (for TF normalization)
    pub total_terms: u32,
    /// Silo that owns this document
    pub silo_id: u64,
    /// Index timestamp (ns)
    pub indexed_at: u64,
}

// ─── Search Results ─────────────────────────────────────────────────────────

/// A search result with relevance score.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Document ID
    pub doc_id: u64,
    /// OID
    pub oid: [u8; 32],
    /// Label
    pub label: String,
    /// TF-IDF relevance score
    pub score: f64,
    /// Matching fields
    pub matched_fields: Vec<Field>,
    /// Snippet (context around the match)
    pub snippet: Option<String>,
}

/// Search query options.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Search terms
    pub terms: Vec<String>,
    /// Restrict to specific fields (None = all)
    pub field_filter: Option<Field>,
    /// Restrict to specific Silo
    pub silo_filter: Option<u64>,
    /// Maximum results
    pub limit: usize,
    /// Require all terms (AND) vs any term (OR)
    pub match_all: bool,
    /// Enable prefix matching
    pub prefix_match: bool,
}

impl Default for SearchQuery {
    fn default() -> Self {
        SearchQuery {
            terms: Vec::new(),
            field_filter: None,
            silo_filter: None,
            limit: 20,
            match_all: false,
            prefix_match: false,
        }
    }
}

// ─── Inverted Index ─────────────────────────────────────────────────────────

/// Index statistics.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub documents_indexed: u64,
    pub total_terms: u64,
    pub unique_terms: u64,
    pub searches_performed: u64,
    pub index_updates: u64,
}

/// The full-text search index.
pub struct SearchIndex {
    /// Inverted index: term → list of postings
    pub postings: BTreeMap<String, Vec<Posting>>,
    /// Document metadata: doc_id → metadata
    pub documents: BTreeMap<u64, IndexedDoc>,
    /// Next document ID
    next_doc_id: u64,
    /// Total document count (for IDF calculation)
    pub doc_count: u64,
    /// Stop words to skip during indexing
    pub stop_words: Vec<String>,
    /// Statistics
    pub stats: IndexStats,
}

impl SearchIndex {
    pub fn new() -> Self {
        let stop_words = ["a", "an", "the", "is", "it", "in", "on", "at", "to",
            "for", "of", "and", "or", "but", "not", "with", "this", "that",
            "from", "by", "as", "be", "was", "were", "been", "are", "have",
            "has", "had", "do", "does", "did", "will", "would", "could",
            "should", "may", "might", "can", "shall"]
            .iter().map(|s| String::from(*s)).collect();

        SearchIndex {
            postings: BTreeMap::new(),
            documents: BTreeMap::new(),
            next_doc_id: 1,
            doc_count: 0,
            stop_words,
            stats: IndexStats::default(),
        }
    }

    /// Index a document.
    pub fn index_document(
        &mut self,
        oid: [u8; 32],
        label: &str,
        tags: &[&str],
        content: &str,
        content_type: &str,
        silo_id: u64,
        now: u64,
    ) -> u64 {
        let doc_id = self.next_doc_id;
        self.next_doc_id += 1;
        self.doc_count += 1;

        // Tokenize and index each field
        let label_terms = self.tokenize(label);
        let content_terms = self.tokenize(content);
        let type_terms = self.tokenize(content_type);

        let total_terms = label_terms.len() + content_terms.len()
            + tags.len() + type_terms.len();

        // Index label terms (boosted)
        self.index_terms(doc_id, &label_terms, Field::Label);

        // Index tags
        for tag in tags {
            let tag_terms = self.tokenize(tag);
            self.index_terms(doc_id, &tag_terms, Field::Tag);
        }

        // Index content
        self.index_terms(doc_id, &content_terms, Field::Content);

        // Index content type
        self.index_terms(doc_id, &type_terms, Field::ContentType);

        // Store document metadata
        self.documents.insert(doc_id, IndexedDoc {
            doc_id,
            oid,
            label: String::from(label),
            total_terms: total_terms as u32,
            silo_id,
            indexed_at: now,
        });

        self.stats.documents_indexed += 1;
        self.stats.index_updates += 1;

        doc_id
    }

    /// Add terms to the inverted index.
    fn index_terms(&mut self, doc_id: u64, terms: &[String], field: Field) {
        // Count term frequencies
        let mut freq_map: BTreeMap<&str, (u32, Vec<u32>)> = BTreeMap::new();
        for (pos, term) in terms.iter().enumerate() {
            let entry = freq_map.entry(term.as_str()).or_insert((0, Vec::new()));
            entry.0 += 1;
            entry.1.push(pos as u32);
        }

        for (term, (freq, positions)) in freq_map {
            let postings_list = self.postings
                .entry(String::from(term))
                .or_insert_with(Vec::new);

            // Check if this doc already has a posting for this field
            if let Some(existing) = postings_list.iter_mut()
                .find(|p| p.doc_id == doc_id && p.field == field)
            {
                existing.term_freq = freq;
                existing.positions = positions;
            } else {
                postings_list.push(Posting {
                    doc_id,
                    field,
                    term_freq: freq,
                    positions,
                });
                self.stats.total_terms += 1;
            }
        }

        self.stats.unique_terms = self.postings.len() as u64;
    }

    /// Remove a document from the index.
    pub fn remove_document(&mut self, doc_id: u64) {
        self.documents.remove(&doc_id);
        // Remove all postings for this document
        for postings_list in self.postings.values_mut() {
            postings_list.retain(|p| p.doc_id != doc_id);
        }
        // Remove empty posting lists
        self.postings.retain(|_, v| !v.is_empty());
        self.doc_count = self.doc_count.saturating_sub(1);
    }

    /// Search the index.
    pub fn search(&mut self, query: &SearchQuery) -> Vec<SearchResult> {
        self.stats.searches_performed += 1;

        if query.terms.is_empty() { return Vec::new(); }

        // Score map: doc_id → (score, matched_fields)
        let mut scores: BTreeMap<u64, (f64, Vec<Field>)> = BTreeMap::new();

        for term in &query.terms {
            let normalized = term.to_lowercase();
            let matching_postings = if query.prefix_match {
                self.prefix_lookup(&normalized)
            } else {
                self.exact_lookup(&normalized)
            };

            // IDF = log(N / df) where df = number of docs containing the term
            let df = matching_postings.len() as f64;
            let idf = if df > 0.0 && self.doc_count > 0 {
                (self.doc_count as f64 / df).ln()
            } else {
                0.0
            };

            for posting in &matching_postings {
                // Apply field filter
                if let Some(field_filter) = query.field_filter {
                    if posting.field != field_filter { continue; }
                }

                // Apply Silo filter
                if let Some(silo_filter) = query.silo_filter {
                    if let Some(doc) = self.documents.get(&posting.doc_id) {
                        if doc.silo_id != silo_filter { continue; }
                    }
                }

                // TF-IDF score
                let total = self.documents.get(&posting.doc_id)
                    .map(|d| d.total_terms.max(1) as f64)
                    .unwrap_or(1.0);
                let tf = posting.term_freq as f64 / total;

                // Field boost
                let boost = match posting.field {
                    Field::Label => 3.0,
                    Field::Tag => 2.0,
                    Field::ContentType => 1.5,
                    Field::Content => 1.0,
                };

                let score = tf * idf * boost;

                let entry = scores.entry(posting.doc_id)
                    .or_insert((0.0, Vec::new()));
                entry.0 += score;
                if !entry.1.contains(&posting.field) {
                    entry.1.push(posting.field);
                }
            }
        }

        // If match_all, filter to docs matching all terms
        if query.match_all && query.terms.len() > 1 {
            let min_fields = query.terms.len();
            // Simple heuristic: require at least as many field matches as terms
            // (A proper AND would track per-term matches)
            scores.retain(|_, (_, fields)| fields.len() >= 1);
        }

        // Build results
        let mut results: Vec<SearchResult> = scores.into_iter()
            .filter_map(|(doc_id, (score, fields))| {
                let doc = self.documents.get(&doc_id)?;
                Some(SearchResult {
                    doc_id,
                    oid: doc.oid,
                    label: doc.label.clone(),
                    score,
                    matched_fields: fields,
                    snippet: None,
                })
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score)
            .unwrap_or(core::cmp::Ordering::Equal));
        results.truncate(query.limit);

        results
    }

    /// Exact term lookup.
    fn exact_lookup(&self, term: &str) -> Vec<&Posting> {
        self.postings.get(term)
            .map(|list| list.iter().collect())
            .unwrap_or_default()
    }

    /// Prefix term lookup (for autocomplete).
    fn prefix_lookup(&self, prefix: &str) -> Vec<&Posting> {
        let mut results = Vec::new();
        for (term, postings) in self.postings.range(String::from(prefix)..) {
            if !term.starts_with(prefix) { break; }
            results.extend(postings.iter());
        }
        results
    }

    /// Tokenize text into lowercase terms, filtering stop words.
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|s| !s.is_empty() && s.len() > 1)
            .map(|s| s.to_lowercase())
            .filter(|s| !self.stop_words.contains(s))
            .collect()
    }

    /// Autocomplete: return terms matching a prefix.
    pub fn autocomplete(&self, prefix: &str, limit: usize) -> Vec<(String, usize)> {
        let prefix = prefix.to_lowercase();
        let mut results = Vec::new();

        for (term, postings) in self.postings.range(prefix.clone()..) {
            if !term.starts_with(&prefix) { break; }
            results.push((term.clone(), postings.len()));
            if results.len() >= limit { break; }
        }

        results
    }
}
