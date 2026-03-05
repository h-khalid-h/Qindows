//! # Synapse Retrieval-Augmented Generation (RAG)
//!
//! Connects Prism's semantic object graph with Synapse's NLP
//! pipeline. When the user asks a question, RAG:
//!   1. Encodes the query into a vector embedding
//!   2. Searches the Prism graph for relevant objects
//!   3. Ranks and filters the results
//!   4. Constructs a context window for the AI model
//!   5. Generates a grounded response
//!
//! This prevents hallucination by anchoring AI responses in
//! the user's actual data.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Retrieval ──────────────────────────────────────────────────────────────

/// A semantic search query.
#[derive(Debug, Clone)]
pub struct RagQuery {
    /// Raw natural language query
    pub text: String,
    /// Query embedding (vector)
    pub embedding: Vec<f32>,
    /// Maximum number of results to retrieve
    pub top_k: usize,
    /// Minimum similarity threshold (0.0 – 1.0)
    pub min_similarity: f32,
    /// Filter by content type (None = all)
    pub content_filter: Option<String>,
    /// Filter by Silo (None = current user's Silo)
    pub silo_filter: Option<u64>,
}

/// A retrieved document chunk.
#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    /// Source object ID (Prism OID)
    pub oid: [u8; 32],
    /// Source label / title
    pub label: String,
    /// Content type (e.g., "document", "email", "note")
    pub content_type: String,
    /// The actual text chunk
    pub text: String,
    /// Chunk's embedding
    pub embedding: Vec<f32>,
    /// Cosine similarity to the query
    pub similarity: f32,
    /// Character offset within the source object
    pub offset: usize,
    /// Chunk length in characters
    pub length: usize,
    /// Timestamp of source object
    pub timestamp: u64,
}

/// Re-ranking strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankStrategy {
    /// Pure cosine similarity (default)
    CosineSimilarity,
    /// Recency-weighted: boost recent documents
    RecencyBoosted,
    /// Diversity: ensure variety in results (MMR)
    MaximalMarginalRelevance,
    /// Cross-encoder re-ranking (more accurate, slower)
    CrossEncoder,
}

// ─── Context Construction ───────────────────────────────────────────────────

/// A context window for the AI model.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    /// System prompt
    pub system_prompt: String,
    /// Retrieved context chunks (formatted)
    pub context: String,
    /// The user's original query
    pub query: String,
    /// Total tokens in the context
    pub token_count: usize,
    /// Maximum tokens allowed
    pub max_tokens: usize,
    /// Sources used (for citation)
    pub sources: Vec<Source>,
}

/// A source citation.
#[derive(Debug, Clone)]
pub struct Source {
    /// Source OID
    pub oid: [u8; 32],
    /// Source label
    pub label: String,
    /// Relevance score
    pub score: f32,
}

// ─── RAG Configuration ─────────────────────────────────────────────────────

/// RAG pipeline configuration.
#[derive(Debug, Clone)]
pub struct RagConfig {
    /// Maximum context window size (tokens)
    pub max_context_tokens: usize,
    /// Number of top results to retrieve
    pub top_k: usize,
    /// Minimum similarity threshold
    pub min_similarity: f32,
    /// Chunk size for splitting documents (characters)
    pub chunk_size: usize,
    /// Chunk overlap (characters)
    pub chunk_overlap: usize,
    /// Re-ranking strategy
    pub rerank: RerankStrategy,
    /// Recency boost factor (for RecencyBoosted strategy)
    pub recency_boost: f32,
    /// MMR lambda (for MaximalMarginalRelevance)
    pub mmr_lambda: f32,
}

impl Default for RagConfig {
    fn default() -> Self {
        RagConfig {
            max_context_tokens: 4096,
            top_k: 10,
            min_similarity: 0.3,
            chunk_size: 512,
            chunk_overlap: 64,
            rerank: RerankStrategy::CosineSimilarity,
            recency_boost: 0.1,
            mmr_lambda: 0.7,
        }
    }
}

// ─── RAG Pipeline ───────────────────────────────────────────────────────────

/// RAG statistics.
#[derive(Debug, Clone, Default)]
pub struct RagStats {
    pub queries_processed: u64,
    pub chunks_retrieved: u64,
    pub chunks_filtered: u64,
    pub contexts_built: u64,
    pub avg_similarity: f64,
    pub total_sources_cited: u64,
}

/// The RAG Pipeline.
pub struct RagPipeline {
    /// Configuration
    pub config: RagConfig,
    /// Document index: chunks pre-split and embedded
    pub index: Vec<RetrievedChunk>,
    /// Statistics
    pub stats: RagStats,
}

impl RagPipeline {
    pub fn new(config: RagConfig) -> Self {
        RagPipeline {
            config,
            index: Vec::new(),
            stats: RagStats::default(),
        }
    }

    /// Index a document: split into chunks and compute embeddings.
    pub fn index_document(
        &mut self,
        oid: [u8; 32],
        label: &str,
        content_type: &str,
        text: &str,
        timestamp: u64,
    ) {
        let chunks = self.split_chunks(text);

        for (offset, chunk) in chunks {
            let embedding = self.compute_embedding(&chunk);
            self.index.push(RetrievedChunk {
                oid,
                label: String::from(label),
                content_type: String::from(content_type),
                text: chunk,
                embedding,
                similarity: 0.0,
                offset,
                length: 0, // Will be set below
                timestamp,
            });
            if let Some(last) = self.index.last_mut() {
                last.length = last.text.len();
            }
        }
    }

    /// Split text into overlapping chunks.
    fn split_chunks(&self, text: &str) -> Vec<(usize, String)> {
        let mut chunks = Vec::new();
        let bytes = text.as_bytes();
        let size = self.config.chunk_size;
        let overlap = self.config.chunk_overlap;
        let step = if size > overlap { size - overlap } else { 1 };

        let mut offset = 0;
        while offset < text.len() {
            let end = (offset + size).min(text.len());
            // Find a safe UTF-8 boundary
            let safe_end = self.find_char_boundary(text, end);
            let chunk = &text[offset..safe_end];
            if !chunk.trim().is_empty() {
                chunks.push((offset, String::from(chunk)));
            }
            offset += step;
            if offset >= text.len() { break; }
            // Make sure we start at a char boundary
            offset = self.find_char_boundary(text, offset);
        }

        chunks
    }

    /// Find a valid UTF-8 character boundary at or before `pos`.
    fn find_char_boundary(&self, text: &str, pos: usize) -> usize {
        let mut p = pos.min(text.len());
        while p > 0 && !text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    /// Compute a simple embedding for a text chunk.
    /// (Placeholder: in production this calls the NPU/Synapse model.)
    fn compute_embedding(&self, text: &str) -> Vec<f32> {
        // Simplified: hash-based embedding (3 dimensions for demo)
        let mut v = [0.0f32; 3];
        for (i, byte) in text.bytes().enumerate() {
            v[i % 3] += byte as f32 * 0.001;
        }
        // Normalize
        let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(0.001);
        alloc::vec![v[0] / mag, v[1] / mag, v[2] / mag]
    }

    /// Retrieve relevant chunks for a query.
    pub fn retrieve(&mut self, query: &RagQuery) -> Vec<RetrievedChunk> {
        self.stats.queries_processed += 1;

        // Score all chunks by cosine similarity
        let mut scored: Vec<RetrievedChunk> = self.index.iter()
            .filter(|chunk| {
                // Apply content filter
                if let Some(ref ct) = query.content_filter {
                    if chunk.content_type != *ct { return false; }
                }
                // Apply Silo filter (would check OID→Silo mapping)
                true
            })
            .map(|chunk| {
                let sim = cosine_similarity(&query.embedding, &chunk.embedding);
                let mut scored_chunk = chunk.clone();
                scored_chunk.similarity = sim;
                scored_chunk
            })
            .filter(|chunk| chunk.similarity >= query.min_similarity)
            .collect();

        // Re-rank
        match self.config.rerank {
            RerankStrategy::RecencyBoosted => {
                // Boost recent documents
                let now_approx = scored.iter().map(|c| c.timestamp).max().unwrap_or(0);
                for chunk in &mut scored {
                    let age_hours = now_approx.saturating_sub(chunk.timestamp) as f32
                        / 3_600_000_000_000.0;
                    let boost = (-self.config.recency_boost * age_hours).exp();
                    chunk.similarity *= boost;
                }
            }
            RerankStrategy::MaximalMarginalRelevance => {
                // MMR: balance relevance and diversity
                scored = self.mmr_rerank(scored, &query.embedding);
            }
            _ => {}
        }

        // Sort by similarity (descending)
        scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity)
            .unwrap_or(core::cmp::Ordering::Equal));
        scored.truncate(query.top_k);

        self.stats.chunks_retrieved += scored.len() as u64;

        // Update average similarity
        if !scored.is_empty() {
            let avg: f64 = scored.iter().map(|c| c.similarity as f64).sum::<f64>()
                / scored.len() as f64;
            self.stats.avg_similarity = (self.stats.avg_similarity * 0.9) + (avg * 0.1);
        }

        scored
    }

    /// Maximal Marginal Relevance re-ranking.
    fn mmr_rerank(&self, candidates: Vec<RetrievedChunk>, query_emb: &[f32]) -> Vec<RetrievedChunk> {
        let lambda = self.config.mmr_lambda;
        let mut selected: Vec<RetrievedChunk> = Vec::new();
        let mut remaining = candidates;

        while !remaining.is_empty() && selected.len() < self.config.top_k {
            let mut best_idx = 0;
            let mut best_score = f32::MIN;

            for (i, cand) in remaining.iter().enumerate() {
                let relevance = cosine_similarity(query_emb, &cand.embedding);

                // Max similarity to any already-selected chunk
                let max_sim = selected.iter()
                    .map(|s| cosine_similarity(&s.embedding, &cand.embedding))
                    .fold(0.0f32, f32::max);

                let mmr_score = lambda * relevance - (1.0 - lambda) * max_sim;
                if mmr_score > best_score {
                    best_score = mmr_score;
                    best_idx = i;
                }
            }

            let winner = remaining.remove(best_idx);
            selected.push(winner);
        }

        selected
    }

    /// Build a context window from retrieved chunks.
    pub fn build_context(&mut self, query: &str, chunks: &[RetrievedChunk]) -> ContextWindow {
        self.stats.contexts_built += 1;

        let mut context_parts = Vec::new();
        let mut sources = Vec::new();
        let mut token_estimate = 0;

        for chunk in chunks {
            // Rough token estimate: ~4 chars per token
            let chunk_tokens = chunk.text.len() / 4;
            if token_estimate + chunk_tokens > self.config.max_context_tokens {
                break;
            }

            context_parts.push(alloc::format!(
                "[Source: {}]\n{}\n",
                chunk.label, chunk.text
            ));
            sources.push(Source {
                oid: chunk.oid,
                label: chunk.label.clone(),
                score: chunk.similarity,
            });
            token_estimate += chunk_tokens;
        }

        self.stats.total_sources_cited += sources.len() as u64;

        ContextWindow {
            system_prompt: String::from(
                "You are a helpful assistant. Answer based on the provided context. \
                 Cite sources when possible."
            ),
            context: context_parts.join("\n"),
            query: String::from(query),
            token_count: token_estimate,
            max_tokens: self.config.max_context_tokens,
            sources,
        }
    }

    /// End-to-end: query → retrieve → build context.
    pub fn query(&mut self, text: &str) -> ContextWindow {
        let embedding = self.compute_embedding(text);
        let query = RagQuery {
            text: String::from(text),
            embedding,
            top_k: self.config.top_k,
            min_similarity: self.config.min_similarity,
            content_filter: None,
            silo_filter: None,
        };

        let chunks = self.retrieve(&query);
        self.build_context(text, &chunks)
    }

    /// Get index size.
    pub fn index_size(&self) -> usize {
        self.index.len()
    }
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 { return 0.0; }

    let mut dot = 0.0f32;
    let mut mag_a = 0.0f32;
    let mut mag_b = 0.0f32;

    for i in 0..len {
        dot += a[i] * b[i];
        mag_a += a[i] * a[i];
        mag_b += b[i] * b[i];
    }

    let denom = (mag_a.sqrt() * mag_b.sqrt()).max(1e-10);
    dot / denom
}
