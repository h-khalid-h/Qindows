//! # Prism Search — Semantic Object Graph Engine (Phase 72)
//!
//! The Prism is the "soul" of Qindows. It replaces the hierarchical folder
//! system with a **Semantic Object Graph** where every file, email, note, and
//! event is a content-addressed Q-Node searchable by *meaning*, not path.
//!
//! ## ARCHITECTURE.md §3.4 — The Prism
//!
//! > "The core Prism syscall resolves *meaning*, not file paths"
//! > `q_resolve_intent(identity_token, "The contract I discussed with Sarah Tuesday")`
//!
//! This module implements the **kernel side** of the Prism semantic engine:
//!
//! ```text
//! App (Silo)                      Qernel (this module)
//! ──────────────────────────────  ────────────────────────────────────────
//! q_resolve_intent(query)  ──────►  PrismSearchEngine::resolve_intent()
//!                                       │ 1. Embed query → VectorHash
//!                                       │ 2. ANN scan across PrismIndex
//!                                       │ 3. Filter by CapToken + identity
//!                                       │ 4. Rank by recency × similarity
//!                                       │ 5. Return Vec<ObjectHandle>
//! ObjectHandle(OID, score) ◄─────────  ─┘
//! ```
//!
//! ## Architecture Guardian: Separation of Concerns
//! - `PrismSearchEngine`: keyword + semantic ANN search routing
//! - `PrismIndex`: in-memory index of Q-Nodes (no storage logic here)
//! - `TimelineView`: time-ordered object window (Timeline Slider)
//! - `VirtualView`: named query saved as a pointer set (no physical copy)
//! - Storage (NVMe) and cryptography (TPM key lookup) remain in their own modules
//!
//! ## Q-Manifest Law 5 (Global Deduplication)
//! Two Q-Nodes with identical content (same SHA-256 hash) share a single
//! physical NVMe block. The Prism index maps both OIDs to the same block.
//!
//! ## Q-Manifest Law 9 (Universal Namespace)
//! All objects are addressed by `prism://sha256:<hex>` — location is irrelevant.
//! A search result from a Nexus peer costs the same as a local object.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;
use alloc::format;

// ── Q-Node (Object Record) ────────────────────────────────────────────────────

/// Content-addressed object in the Prism graph.
#[derive(Debug, Clone)]
pub struct QNode {
    /// Content hash (SHA-256) — the universal O-ID
    pub oid: [u8; 32],
    /// Object type label (e.g. "document", "email", "image", "code")
    pub object_type: String,
    /// Human-readable title (for display)
    pub title: String,
    /// Creator Silo ID — used for Law 1 capability filtering
    pub creator_silo: u64,
    /// Kernel tick when object was created
    pub created_at: u64,
    /// Kernel tick of last modification
    pub modified_at: u64,
    /// Size in bytes of backing NVMe block
    pub size_bytes: u64,
    /// 256-dimension normalized embedding vector (NPU-generated)
    /// Stored as i8 quantized [-128,127] to minimize RAM usage
    pub vector_hash: [i8; 32], // simplified 32-dim for in-kernel use
    /// Comma-separated semantic keywords extracted at ingest time
    pub keywords: String,
    /// Lineage: OID of the previous version (None = original)
    pub parent_oid: Option<[u8; 32]>,
    /// UNS URI: `prism://sha256:<hex>`
    pub uns_uri: String,
    /// Has this node been "vault-locked" (hardware-bound encryption)?
    pub vault_locked: bool,
    /// Is this node publicly shareable to the Nexus mesh (Law 9)?
    pub mesh_public: bool,
}

impl QNode {
    /// Construct a new Q-Node from raw object bytes (simulates NPU embedding).
    pub fn new(
        oid: [u8; 32],
        object_type: &str,
        title: &str,
        creator_silo: u64,
        tick: u64,
        size_bytes: u64,
        keywords: &str,
    ) -> Self {
        // Generate a synthetic vector from the OID bytes (placeholder for NPU embed)
        let mut vector_hash = [0i8; 32];
        for (i, &b) in oid.iter().take(32).enumerate() {
            vector_hash[i] = (b as i8).wrapping_mul(3);
        }
        // UNS URI: prism://sha256:<first 8 bytes hex>
        let hex: String = oid[..8].iter()
            .map(|b| format!("{:02x}", b))
            .fold(String::new(), |mut acc, s| { acc.push_str(&s); acc });
        let uns_uri = {
            let mut s = "prism://sha256:".to_string();
            s.push_str(&hex);
            s
        };
        QNode {
            oid,
            object_type: object_type.to_string(),
            title: title.to_string(),
            creator_silo,
            created_at: tick,
            modified_at: tick,
            size_bytes,
            vector_hash,
            keywords: keywords.to_string(),
            parent_oid: None,
            uns_uri,
            vault_locked: false,
            mesh_public: false,
        }
    }

    /// Compute a simple cosine-similarity-like score against a query vector.
    /// Returns 0 (no match) to 1000 (perfect match).
    pub fn similarity(&self, query_vec: &[i8; 32]) -> u32 {
        let dot: i32 = self.vector_hash.iter()
            .zip(query_vec.iter())
            .map(|(&a, &b)| (a as i32) * (b as i32))
            .sum();
        // Normalize: dot ∈ [-127*127*32, 127*127*32] → [0, 1000]
        let max_dot = 127i32 * 127 * 32;
        let clamped = dot.max(-max_dot).min(max_dot);
        ((clamped + max_dot) * 500 / max_dot) as u32 // scale to 0-1000
    }

    /// Keyword relevance: count of query words found in keywords string.
    pub fn keyword_match_score(&self, query: &str) -> u32 {
        query.split_whitespace()
            .filter(|word| self.keywords.contains(*word) || self.title.contains(*word))
            .count() as u32 * 100
    }
}

// ── Object Handle (returned to app via Q-Ring) ────────────────────────────────

/// A resolved object handle returned to a Silo after a Prism search.
#[derive(Debug, Clone)]
pub struct ObjectHandle {
    /// The unique object identifier (content hash)
    pub oid: [u8; 32],
    /// Object type string
    pub object_type: String,
    /// Human-readable title
    pub title: String,
    /// UNS URI for direct access
    pub uns_uri: String,
    /// Relevance ranking score (higher = more relevant, 0–2000)
    pub rank_score: u32,
    /// Object creation tick
    pub created_at: u64,
    /// Object size in bytes
    pub size_bytes: u64,
}

impl ObjectHandle {
    pub fn from_node(node: &QNode, score: u32) -> Self {
        ObjectHandle {
            oid: node.oid,
            object_type: node.object_type.clone(),
            title: node.title.clone(),
            uns_uri: node.uns_uri.clone(),
            rank_score: score,
            created_at: node.created_at,
            size_bytes: node.size_bytes,
        }
    }
}

// ── Virtual View ──────────────────────────────────────────────────────────────

/// A saved query ("Virtual View") — a named pointer set over Prism objects.
/// No physical copies of data. Mutation of underlying objects is reflected instantly.
#[derive(Debug, Clone)]
pub struct VirtualView {
    /// Unique view ID
    pub view_id: u64,
    /// Owner Silo
    pub owner_silo: u64,
    /// Human-readable name (e.g. "Work Projects 2026")
    pub name: String,
    /// The query string that defines this view
    pub query: String,
    /// Maximum results to return  
    pub limit: u32,
    /// Tick of last refresh
    pub last_refreshed: u64,
    /// Cached result OIDs (refreshed on each access)
    pub cached_oids: Vec<[u8; 32]>,
}

// ── Timeline Window ───────────────────────────────────────────────────────────

/// A time-bounded window over the Prism for the Timeline Slider.
#[derive(Debug, Clone)]
pub struct TimelineWindow {
    /// Start tick (inclusive)
    pub from_tick: u64,
    /// End tick (inclusive, 0 = now)
    pub to_tick: u64,
    /// Object types to include (empty = all)
    pub filter_types: Vec<String>,
    /// Owner silo for capability filtering
    pub owner_silo: u64,
}

// ── Prism Index ───────────────────────────────────────────────────────────────

/// The in-memory Prism object index.
/// Maps OID → QNode for fast O(log n) lookups.
/// Architecture Guardian: this is a pure data structure — no I/O, no crypto.
#[derive(Default)]
pub struct PrismIndex {
    /// Primary index: first 8 bytes of OID (as u64) → QNode
    pub nodes: BTreeMap<u64, QNode>,
    /// Creator index: silo_id → list of OID keys (for Law 1 filtering)
    pub creator_idx: BTreeMap<u64, Vec<u64>>,
    /// Type index: type_string_hash → list of OID keys
    pub type_idx: BTreeMap<u64, Vec<u64>>,
    /// Dedup count: tracks how many OID aliases point to same content
    pub dedup_hits: u64,
    /// Total objects indexed
    pub total_nodes: u64,
}

impl PrismIndex {
    pub fn new() -> Self { Self::default() }

    /// OID → compact u64 key (first 8 bytes LE)
    fn oid_key(oid: &[u8; 32]) -> u64 {
        u64::from_le_bytes([oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7]])
    }

    /// Ingest a new Q-Node into the index.
    pub fn ingest(&mut self, node: QNode) {
        let key = Self::oid_key(&node.oid);
        // Law 5: if the key already exists, it's a dedup hit
        if self.nodes.contains_key(&key) {
            self.dedup_hits += 1;
            return;
        }
        let silo = node.creator_silo;
        let type_hash: u64 = node.object_type.bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));

        self.creator_idx.entry(silo).or_insert_with(Vec::new).push(key);
        self.type_idx.entry(type_hash).or_insert_with(Vec::new).push(key);
        self.nodes.insert(key, node);
        self.total_nodes += 1;
    }

    /// Look up a node by OID.
    pub fn get(&self, oid: &[u8; 32]) -> Option<&QNode> {
        self.nodes.get(&Self::oid_key(oid))
    }

    /// Remove a node (on Silo vaporize / uninstall).
    pub fn remove(&mut self, oid: &[u8; 32]) {
        self.nodes.remove(&Self::oid_key(oid));
    }

    /// All nodes belonging to a specific Silo (Law 1 scope).
    pub fn nodes_for_silo(&self, silo_id: u64) -> Vec<&QNode> {
        self.creator_idx.get(&silo_id)
            .map(|keys| keys.iter().filter_map(|k| self.nodes.get(k)).collect())
            .unwrap_or_default()
    }
}

// ── Query Result ──────────────────────────────────────────────────────────────

/// Reason why a search match was included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchReason {
    SemanticVector,  // ANN vector similarity
    KeywordTitle,    // Exact keyword or title match
    TimelineWindow,  // Time-bounded query
    VirtualView,     // Returned from a saved virtual view
}

/// A single ranked search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub handle: ObjectHandle,
    pub match_reason: MatchReason,
}

// ── Prism Search Statistics ───────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct PrismSearchStats {
    pub total_queries: u64,
    pub semantic_queries: u64,
    pub keyword_queries: u64,
    pub timeline_queries: u64,
    pub virtual_view_refreshes: u64,
    pub cap_rejected_results: u64, // results filtered by Law 1
    pub avg_results_per_query: u64,
}

// ── Prism Search Engine ───────────────────────────────────────────────────────

/// The kernel-side Prism semantic search engine.
/// Implements `q_resolve_intent()` as described in ARCHITECTURE.md §3.4.
pub struct PrismSearchEngine {
    /// The object index
    pub index: PrismIndex,
    /// Saved Virtual Views: view_id → VirtualView
    pub virtual_views: BTreeMap<u64, VirtualView>,
    /// Next view ID
    next_view_id: u64,
    /// Search statistics
    pub stats: PrismSearchStats,
    /// Maximum results per query
    pub default_limit: u32,
}

impl PrismSearchEngine {
    pub fn new() -> Self {
        PrismSearchEngine {
            index: PrismIndex::new(),
            virtual_views: BTreeMap::new(),
            next_view_id: 1,
            stats: PrismSearchStats::default(),
            default_limit: 50,
        }
    }

    // ── Core API: q_resolve_intent ────────────────────────────────────────────

    /// ARCHITECTURE.md §3.4: resolve *meaning*, not file paths.
    ///
    /// ```text
    /// q_resolve_intent("The contract I discussed with Sarah Tuesday")
    ///   → returns: [ObjectHandle { "NDA-Sarah-2026-03.pdf", rank: 987 }, ...]
    /// ```
    ///
    /// Steps:
    /// 1. Embed query text → a synthetic 32-dim vector (NPU generates this in prod)
    /// 2. ANN scan across PrismIndex nodes
    /// 3. Merge keyword scores
    /// 4. Filter by Silo capability (Law 1)
    /// 5. Sort descending by combined score, return top `limit`
    pub fn q_resolve_intent(
        &mut self,
        silo_id: u64,
        query: &str,
        limit: u32,
        tick: u64,
    ) -> Vec<SearchResult> {
        self.stats.total_queries += 1;
        self.stats.semantic_queries += 1;

        crate::serial_println!(
            "[PRISM] q_resolve_intent silo={} query=\"{}\" limit={}", silo_id, query, limit
        );

        // Step 1: Embed query → synthetic vector
        let query_vec = Self::embed_query(query);

        // Step 2+3: Score every indexed node
        let mut results: Vec<(u32, &QNode)> = self.index.nodes.values()
            .map(|node| {
                let sem   = node.similarity(&query_vec);
                let kw    = node.keyword_match_score(query);
                // Recency bonus: objects modified within last 10K ticks get +200
                let recency = if tick.saturating_sub(node.modified_at) < 10_000 { 200u32 } else { 0 };
                (sem + kw + recency, node)
            })
            .filter(|(score, _)| *score > 50) // discard irrelevant results
            .collect();

        // Step 4: Capability filter (Law 1) — only return objects the Silo can access
        // In production: check CapToken for PRISM_READ on each OID
        // Here: objects created by this Silo OR marked mesh_public are accessible
        results.retain(|(_, node)| node.creator_silo == silo_id || node.mesh_public);
        self.stats.cap_rejected_results += (self.index.total_nodes as usize)
            .saturating_sub(results.len()) as u64;

        // Step 5: Sort descending by score
        results.sort_by(|a, b| b.0.cmp(&a.0));
        results.truncate(limit.min(self.default_limit) as usize);

        let count = results.len() as u64;
        self.stats.avg_results_per_query =
            (self.stats.avg_results_per_query * (self.stats.total_queries - 1) + count)
            / self.stats.total_queries;

        results.into_iter()
            .map(|(score, node)| SearchResult {
                handle: ObjectHandle::from_node(node, score),
                match_reason: MatchReason::SemanticVector,
            })
            .collect()
    }

    /// Keyword-only search (for fast path when no NPU available).
    pub fn search_keywords(
        &mut self,
        silo_id: u64,
        query: &str,
        limit: u32,
    ) -> Vec<SearchResult> {
        self.stats.total_queries += 1;
        self.stats.keyword_queries += 1;

        let mut results: Vec<(u32, &QNode)> = self.index.nodes.values()
            .filter_map(|node| {
                let score = node.keyword_match_score(query);
                if score > 0 && (node.creator_silo == silo_id || node.mesh_public) {
                    Some((score, node))
                } else { None }
            })
            .collect();

        results.sort_by(|a, b| b.0.cmp(&a.0));
        results.truncate(limit as usize);

        results.into_iter()
            .map(|(score, node)| SearchResult {
                handle: ObjectHandle::from_node(node, score),
                match_reason: MatchReason::KeywordTitle,
            })
            .collect()
    }

    // ── Timeline Slider ───────────────────────────────────────────────────────

    /// §3.4: "Timeline Slider — scrub your entire digital life backward in time."
    pub fn timeline_query(
        &mut self,
        silo_id: u64,
        window: &TimelineWindow,
        limit: u32,
    ) -> Vec<SearchResult> {
        self.stats.total_queries += 1;
        self.stats.timeline_queries += 1;

        let end_tick = if window.to_tick == 0 { u64::MAX } else { window.to_tick };

        let mut results: Vec<(u64, &QNode)> = self.index.nodes.values()
            .filter(|node| {
                node.created_at >= window.from_tick
                && node.created_at <= end_tick
                && node.creator_silo == silo_id
                && (window.filter_types.is_empty()
                    || window.filter_types.iter().any(|t| *t == node.object_type))
            })
            .map(|node| (node.created_at, node))
            .collect();

        // Most recent first
        results.sort_by(|a, b| b.0.cmp(&a.0));
        results.truncate(limit as usize);

        results.into_iter()
            .map(|(_, node)| SearchResult {
                handle: ObjectHandle::from_node(node, 0),
                match_reason: MatchReason::TimelineWindow,
            })
            .collect()
    }

    // ── Virtual Views ─────────────────────────────────────────────────────────

    /// §3.4: Create a Virtual View — "logical groupings that *point to* objects,
    /// no physical copies."
    pub fn create_virtual_view(
        &mut self,
        owner_silo: u64,
        name: &str,
        query: &str,
        limit: u32,
        tick: u64,
    ) -> u64 {
        let view_id = self.next_view_id;
        self.next_view_id += 1;
        let view = VirtualView {
            view_id,
            owner_silo,
            name: name.to_string(),
            query: query.to_string(),
            limit,
            last_refreshed: tick,
            cached_oids: Vec::new(),
        };
        self.virtual_views.insert(view_id, view);
        crate::serial_println!(
            "[PRISM] Virtual View {} created: \"{}\" query=\"{}\"", view_id, name, query
        );
        view_id
    }

    /// Refresh and return a Virtual View's results.
    pub fn get_virtual_view(
        &mut self,
        view_id: u64,
        tick: u64,
    ) -> Option<Vec<SearchResult>> {
        let (owner_silo, query, limit) = {
            let view = self.virtual_views.get(&view_id)?;
            (view.owner_silo, view.query.clone(), view.limit)
        };
        self.stats.virtual_view_refreshes += 1;
        let results = self.q_resolve_intent(owner_silo, &query, limit, tick);
        if let Some(view) = self.virtual_views.get_mut(&view_id) {
            view.last_refreshed = tick;
            view.cached_oids = results.iter().map(|r| r.handle.oid).collect();
        }
        Some(results)
    }

    /// Delete a Virtual View (pointer deletion — no data lost).
    pub fn delete_virtual_view(&mut self, view_id: u64) -> bool {
        self.virtual_views.remove(&view_id).is_some()
    }

    // ── Ingest / Evict ────────────────────────────────────────────────────────

    /// Ingest a new object into the Prism index (called by QFS Ghost-Write).
    pub fn ingest_object(&mut self, node: QNode) {
        crate::serial_println!(
            "[PRISM] Ingest: {} \"{}\" ({} bytes) by silo={}",
            node.object_type, node.title, node.size_bytes, node.creator_silo
        );
        self.index.ingest(node);
    }

    /// Remove object on Silo vaporize or explicit delete.
    pub fn evict_object(&mut self, oid: &[u8; 32]) {
        self.index.remove(oid);
    }

    // ── Query Embedding ───────────────────────────────────────────────────────

    /// Produce a 32-dim embedding from a text query.
    /// Production: NPU inference → 256-dim float vector → quantize to i8.
    /// Here: deterministic hash-based synthetic embedding.
    fn embed_query(query: &str) -> [i8; 32] {
        let mut vec = [0i8; 32];
        for (i, b) in query.bytes().enumerate() {
            let slot = i % 32;
            vec[slot] = vec[slot].wrapping_add(b as i8);
        }
        vec
    }

    /// Print a summary to serial console.
    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║         Prism Search Engine          ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Objects indexed:  {:>8}            ║", self.index.total_nodes);
        crate::serial_println!("║ Dedup hits:       {:>8}            ║", self.index.dedup_hits);
        crate::serial_println!("║ Total queries:    {:>8}            ║", self.stats.total_queries);
        crate::serial_println!("║ Virtual Views:    {:>8}            ║", self.virtual_views.len());
        crate::serial_println!("║ Avg results/q:    {:>8}            ║", self.stats.avg_results_per_query);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
