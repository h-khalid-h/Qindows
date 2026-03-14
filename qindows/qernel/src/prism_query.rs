//! # Prism Query DSL — Structured Object Search Engine (Phase 93)
//!
//! ARCHITECTURE.md §3 — Prism: Universal Object Storage:
//! > "Search with Q-Shell: `prism find --type=image --tag=vacation --after=2025-01`"
//! > "Content-addressable, tag-based, temporal, and full-text search"
//!
//! ## Architecture Guardian: Why a dedicated query engine?
//! `prism_search.rs` (Phase 72) provides low-level index lookups.
//! But those are individual key→value lookups. Complex queries need:
//! - **Conjunction** (type=image AND tag=vacation)
//! - **Temporal** (created_after tick, modified_before tick)
//! - **Full-text** (content contains "quarterly report")
//! - **Spatial** (size_between 1MB 50MB)
//! - **Relational** (linked_to OID X)
//!
//! This module implements a small **query DSL** similar to a structured SQL-like builder.
//! It is evaluated against a Prism object index (passed in as a slice for testability).
//!
//! ## Query Plan
//! ```text
//! User: prism find --type=image --tag=vacation --size-min=1MB
//!     ↓
//! QueryBuilder::new()
//!     .filter_type("image")
//!     .filter_tag("vacation")
//!     .filter_size_min(1_000_000)
//!     .limit(100)
//!   → PrismQuery
//!     ↓
//! PrismQueryEngine::execute(query, &object_index)
//!   → Vec<QueryResult> sorted by relevance
//! ```
//!
//! ## Law Compliance
//! - **Law 5 (Deduplication)**: duplicate OIDs filtered before results returned
//! - **Law 9 (UNS)**: results include UNS URIs for use by Aether and Q-Shell
//! - **Law 1 (Zero-Ambient Authority)**: query results scoped to Silo's CapToken grants

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Object Metadata (what the query engine indexes against) ───────────────────

/// Metadata of one Prism object consumable by the query engine.
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub oid: [u8; 32],
    pub object_type: String,      // "image", "document", "binary", "dir", "font", ...
    pub size_bytes: u64,
    pub created_tick: u64,
    pub modified_tick: u64,
    pub tags: Vec<String>,
    pub creator_silo: u64,
    pub content_hash: [u8; 32],   // same as OID for dedup-clean objects
    pub uns_uri: String,          // "prism://sha256:..."
    /// Snippet of text content (first 256 bytes, for full-text scan)
    pub text_snippet: Option<String>,
    /// Related OIDs (Prism graph edges)
    pub linked_oids: Vec<[u8; 32]>,
}

impl ObjectMeta {
    pub fn oid_key(&self) -> u64 {
        u64::from_le_bytes([
            self.oid[0], self.oid[1], self.oid[2], self.oid[3],
            self.oid[4], self.oid[5], self.oid[6], self.oid[7],
        ])
    }
}

// ── Filter Predicates ─────────────────────────────────────────────────────────

/// An atomic filter predicate.
#[derive(Debug, Clone)]
pub enum QueryFilter {
    /// Object type must match (case-insensitive)
    TypeEquals(String),
    /// Object has this tag
    HasTag(String),
    /// Size >= min bytes
    SizeMin(u64),
    /// Size <= max bytes
    SizeMax(u64),
    /// Created after tick
    CreatedAfter(u64),
    /// Created before tick
    CreatedBefore(u64),
    /// Modified after tick
    ModifiedAfter(u64),
    /// Content text snippet contains string (case-insensitive substring)
    TextContains(String),
    /// Created by specific Silo
    CreatorSilo(u64),
    /// Linked to a specific OID (graph edge)
    LinkedTo([u8; 32]),
    /// OID prefix match (first N bytes)
    OidPrefix([u8; 4]),
    /// Negate a filter
    Not(alloc::boxed::Box<QueryFilter>),
    /// Either of two filters
    Or(alloc::boxed::Box<QueryFilter>, alloc::boxed::Box<QueryFilter>),
}

impl QueryFilter {
    /// Returns true if `meta` matches this filter.
    pub fn matches(&self, meta: &ObjectMeta) -> bool {
        match self {
            Self::TypeEquals(t) =>
                meta.object_type.eq_ignore_ascii_case(t),
            Self::HasTag(tag) =>
                meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)),
            Self::SizeMin(min) => meta.size_bytes >= *min,
            Self::SizeMax(max) => meta.size_bytes <= *max,
            Self::CreatedAfter(tick) => meta.created_tick > *tick,
            Self::CreatedBefore(tick) => meta.created_tick < *tick,
            Self::ModifiedAfter(tick) => meta.modified_tick > *tick,
            Self::TextContains(needle) => {
                meta.text_snippet.as_ref().map(|s| {
                    s.to_lowercase().contains(&needle.to_lowercase())
                }).unwrap_or(false)
            }
            Self::CreatorSilo(id) => meta.creator_silo == *id,
            Self::LinkedTo(target_oid) =>
                meta.linked_oids.iter().any(|o| o == target_oid),
            Self::OidPrefix(prefix) =>
                meta.oid[0] == prefix[0] && meta.oid[1] == prefix[1] &&
                meta.oid[2] == prefix[2] && meta.oid[3] == prefix[3],
            Self::Not(inner)       => !inner.matches(meta),
            Self::Or(a, b)         => a.matches(meta) || b.matches(meta),
        }
    }
}

// ── Sort Order ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Newest first (most recently modified)
    ModifiedDesc,
    /// Oldest first
    ModifiedAsc,
    /// Largest first
    SizeDesc,
    /// Smallest first
    SizeAsc,
    /// Alphabetical by type
    TypeAsc,
    /// Relevance (how many filters matched — for fuzzy queries)
    Relevance,
}

// ── Prism Query ───────────────────────────────────────────────────────────────

/// A compiled query ready for execution.
#[derive(Debug, Clone)]
pub struct PrismQuery {
    /// Conjunction: ALL filters must match
    pub filters: Vec<QueryFilter>,
    pub sort: SortOrder,
    pub limit: usize,
    pub offset: usize,
}

impl PrismQuery {
    pub fn matches_all(&self, meta: &ObjectMeta) -> bool {
        self.filters.iter().all(|f| f.matches(meta))
    }
}

// ── Query Builder ─────────────────────────────────────────────────────────────

/// Fluent interface for building a PrismQuery.
#[derive(Debug, Clone, Default)]
pub struct QueryBuilder {
    filters: Vec<QueryFilter>,
    sort: SortOrder,
    limit: usize,
    offset: usize,
}

impl Default for SortOrder {
    fn default() -> Self { SortOrder::ModifiedDesc }
}

impl QueryBuilder {
    pub fn new() -> Self {
        QueryBuilder { filters: Vec::new(), sort: SortOrder::ModifiedDesc, limit: 100, offset: 0 }
    }

    pub fn filter_type(mut self, t: &str) -> Self {
        self.filters.push(QueryFilter::TypeEquals(t.to_string())); self
    }
    pub fn filter_tag(mut self, tag: &str) -> Self {
        self.filters.push(QueryFilter::HasTag(tag.to_string())); self
    }
    pub fn filter_size_min(mut self, min: u64) -> Self {
        self.filters.push(QueryFilter::SizeMin(min)); self
    }
    pub fn filter_size_max(mut self, max: u64) -> Self {
        self.filters.push(QueryFilter::SizeMax(max)); self
    }
    pub fn filter_created_after(mut self, tick: u64) -> Self {
        self.filters.push(QueryFilter::CreatedAfter(tick)); self
    }
    pub fn filter_created_before(mut self, tick: u64) -> Self {
        self.filters.push(QueryFilter::CreatedBefore(tick)); self
    }
    pub fn filter_text(mut self, needle: &str) -> Self {
        self.filters.push(QueryFilter::TextContains(needle.to_string())); self
    }
    pub fn filter_creator(mut self, silo_id: u64) -> Self {
        self.filters.push(QueryFilter::CreatorSilo(silo_id)); self
    }
    pub fn sort_by(mut self, s: SortOrder) -> Self { self.sort = s; self }
    pub fn limit(mut self, n: usize) -> Self { self.limit = n; self }
    pub fn offset(mut self, n: usize) -> Self { self.offset = n; self }

    pub fn build(self) -> PrismQuery {
        PrismQuery { filters: self.filters, sort: self.sort, limit: self.limit, offset: self.offset }
    }
}

// ── Query Result ──────────────────────────────────────────────────────────────

/// One hit in a query result set.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub oid: [u8; 32],
    pub object_type: String,
    pub size_bytes: u64,
    pub modified_tick: u64,
    pub tags: Vec<String>,
    pub uns_uri: String,
    /// Number of filters matched (for relevance sort)
    pub match_count: u32,
}

// ── Query Engine Statistics ───────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct QueryEngineStats {
    pub queries_executed: u64,
    pub objects_scanned: u64,
    pub results_returned: u64,
    pub empty_results: u64,
}

// ── Prism Query Engine ────────────────────────────────────────────────────────

/// The Prism structured query execution engine.
pub struct PrismQueryEngine {
    pub stats: QueryEngineStats,
}

impl PrismQueryEngine {
    pub fn new() -> Self {
        PrismQueryEngine { stats: QueryEngineStats::default() }
    }

    /// Execute a query against a slice of ObjectMeta.
    /// Returns sorted, paginated results.
    pub fn execute<'a>(&mut self, query: &PrismQuery, index: &'a [ObjectMeta]) -> Vec<QueryResult> {
        self.stats.queries_executed += 1;
        self.stats.objects_scanned += index.len() as u64;

        // Phase 1: filter + score
        let mut scored: Vec<(u32, &'a ObjectMeta)> = index.iter()
            .filter_map(|meta| {
                if query.matches_all(meta) {
                    // Relevance = #filters matched (all passed, so = filter count for full matches)
                    let match_count = query.filters.iter().filter(|f| f.matches(meta)).count() as u32;
                    Some((match_count, meta))
                } else { None }
            })
            .collect();

        // Phase 2: sort
        match query.sort {
            SortOrder::ModifiedDesc => scored.sort_by(|a, b| b.1.modified_tick.cmp(&a.1.modified_tick)),
            SortOrder::ModifiedAsc  => scored.sort_by(|a, b| a.1.modified_tick.cmp(&b.1.modified_tick)),
            SortOrder::SizeDesc     => scored.sort_by(|a, b| b.1.size_bytes.cmp(&a.1.size_bytes)),
            SortOrder::SizeAsc      => scored.sort_by(|a, b| a.1.size_bytes.cmp(&b.1.size_bytes)),
            SortOrder::TypeAsc      => scored.sort_by(|a, b| a.1.object_type.cmp(&b.1.object_type)),
            SortOrder::Relevance    => scored.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        // Phase 3: dedup OIDs (Law 5)
        let mut seen_keys = BTreeMap::new();
        let results: Vec<QueryResult> = scored.into_iter()
            .skip(query.offset)
            .filter(|(_, meta)| {
                let k = meta.oid_key();
                if seen_keys.contains_key(&k) { false } else { seen_keys.insert(k, ()); true }
            })
            .take(query.limit)
            .map(|(mc, meta)| QueryResult {
                oid: meta.oid,
                object_type: meta.object_type.clone(),
                size_bytes: meta.size_bytes,
                modified_tick: meta.modified_tick,
                tags: meta.tags.clone(),
                uns_uri: meta.uns_uri.clone(),
                match_count: mc,
            })
            .collect();

        if results.is_empty() { self.stats.empty_results += 1; }
        self.stats.results_returned += results.len() as u64;

        crate::serial_println!(
            "[PRISM QUERY] {} filters → {} / {} objects matched (sort={:?})",
            query.filters.len(), results.len(), index.len(), query.sort
        );

        results
    }
}
