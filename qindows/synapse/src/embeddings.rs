//! # Synapse Embedding Search
//!
//! Semantic search over Prism objects using vector embeddings.
//! Every file, note, and message can be embedded into a high-dimensional
//! vector space. "Find my presentation about quantum computing" works
//! even if no file is named "quantum computing."

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Embedding dimension (256-d for efficiency).
pub const EMBED_DIM: usize = 256;

/// A vector embedding.
#[derive(Debug, Clone)]
pub struct Embedding {
    /// The vector values
    pub values: Vec<f32>,
    /// Source object OID
    pub oid: u64,
    /// Content type
    pub content_type: ContentType,
    /// Text snippet (first ~100 chars for display)
    pub snippet: String,
}

/// Content types that can be embedded.
#[derive(Debug, Clone, Copy)]
pub enum ContentType {
    Document,
    Image,
    Email,
    Message,
    Note,
    Code,
    AudioTranscript,
    WebPage,
}

impl Embedding {
    /// Compute cosine similarity with another embedding.
    pub fn similarity(&self, other: &Embedding) -> f32 {
        if self.values.len() != other.values.len() { return 0.0; }

        let mut dot: f32 = 0.0;
        let mut norm_a: f32 = 0.0;
        let mut norm_b: f32 = 0.0;

        for i in 0..self.values.len() {
            dot += self.values[i] * other.values[i];
            norm_a += self.values[i] * self.values[i];
            norm_b += other.values[i] * other.values[i];
        }

        let denom = norm_a.sqrt() * norm_b.sqrt();
        if denom < 1e-8 { 0.0 } else { dot / denom }
    }
}

/// A search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Source object OID
    pub oid: u64,
    /// Similarity score (0.0 - 1.0)
    pub score: f32,
    /// Content type
    pub content_type: ContentType,
    /// Text snippet
    pub snippet: String,
}

/// The Embedding Index — stores and searches vector embeddings.
pub struct EmbeddingIndex {
    /// All stored embeddings
    pub embeddings: Vec<Embedding>,
    /// Index statistics
    pub stats: IndexStats,
}

/// Index statistics.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub total_embeddings: u64,
    pub total_searches: u64,
    pub avg_search_results: f32,
}

impl EmbeddingIndex {
    pub fn new() -> Self {
        EmbeddingIndex {
            embeddings: Vec::new(),
            stats: IndexStats::default(),
        }
    }

    /// Add an embedding to the index.
    pub fn insert(&mut self, embedding: Embedding) {
        self.stats.total_embeddings += 1;
        self.embeddings.push(embedding);
    }

    /// Search for the top-K most similar embeddings to a query.
    pub fn search(&mut self, query: &Embedding, top_k: usize, min_score: f32) -> Vec<SearchResult> {
        self.stats.total_searches += 1;

        let mut results: Vec<SearchResult> = self.embeddings.iter()
            .map(|e| SearchResult {
                oid: e.oid,
                score: query.similarity(e),
                content_type: e.content_type,
                snippet: e.snippet.clone(),
            })
            .filter(|r| r.score >= min_score)
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(core::cmp::Ordering::Equal));
        results.truncate(top_k);

        self.stats.avg_search_results = (self.stats.avg_search_results
            * (self.stats.total_searches - 1) as f32
            + results.len() as f32) / self.stats.total_searches as f32;

        results
    }

    /// Remove an embedding by OID.
    pub fn remove(&mut self, oid: u64) {
        self.embeddings.retain(|e| e.oid != oid);
        self.stats.total_embeddings = self.embeddings.len() as u64;
    }

    /// Generate a simple bag-of-words embedding from text.
    ///
    /// Production would use a proper neural encoder, but this provides
    /// basic keyword-level semantic matching.
    pub fn embed_text(&self, text: &str, oid: u64, content_type: ContentType) -> Embedding {
        let mut values = alloc::vec![0.0f32; EMBED_DIM];
        let lower = text.to_lowercase();

        // Simple hash-based feature extraction
        for word in lower.split_whitespace() {
            let mut hash: u32 = 0x811c9dc5; // FNV offset
            for byte in word.bytes() {
                hash ^= byte as u32;
                hash = hash.wrapping_mul(0x01000193); // FNV prime
            }

            // Scatter the word's contribution across multiple dimensions
            let dim1 = (hash % EMBED_DIM as u32) as usize;
            let dim2 = ((hash >> 8) % EMBED_DIM as u32) as usize;
            let dim3 = ((hash >> 16) % EMBED_DIM as u32) as usize;

            values[dim1] += 1.0;
            values[dim2] += 0.5;
            values[dim3] += 0.25;
        }

        // L2 normalize
        let norm: f32 = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-8 {
            for v in &mut values {
                *v /= norm;
            }
        }

        let snippet = if text.len() > 100 {
            let mut s = String::from(&text[..100]);
            s.push_str("...");
            s
        } else {
            String::from(text)
        };

        Embedding {
            values,
            oid,
            content_type,
            snippet,
        }
    }

    /// Get index size in approximate bytes.
    pub fn memory_bytes(&self) -> usize {
        self.embeddings.len() * (EMBED_DIM * 4 + 128) // ~1 KiB per embedding
    }
}
