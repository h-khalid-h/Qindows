//! # Timeline Slider — Ghost-Write Version History Navigator (Phase 88)
//!
//! ARCHITECTURE.md §3 — Prism: Timeline Slider:
//! > "Every write generates a Shadow Object — instant version history"
//! > "Slide back in time to any previous version of any object"
//! > "UI: Aether displays a horizontal timeline scrubber per-object"
//!
//! ## Architecture Guardian: Responsibilities
//! `ghost_write_engine.rs` (Phase 86) creates and manages Shadow Objects.
//! `prism_search.rs` (Phase 72) finds objects by OID/content.
//!
//! **This module** provides the **Timeline navigation layer**:
//! - Given an object's current OID, walk backwards through the Shadow Object chain
//! - Build a timeline of versions for Aether's UI scrubber
//! - Perform a "restore" (create a Ghost-Write transaction that makes the old version current)
//! - Provide diff metadata between adjacent versions (size delta, tick delta)
//!
//! ## User-Visible Feature
//! ```text
//! Aether: User right-clicks a document in Prism → "View History"
//! ┌─────────────────────────────────────────────────────┐
//! │ File.doc  ← ─────────────────────────── → v12 (now) │
//! │                   Timeline Slider                     │
//! │  v1  v2  v3  v4  v5  v6  v7  v8  v9  v10  v11  v12 │
//! │  ●───●───●───●───●───●───●───●───●────●────●────●   │
//! │  2min ago               45sec ago         now       │
//! └─────────────────────────────────────────────────────┘
//! Drag → preview old version in-place (no restore needed)
//! Double-click → "Restore" creates a new Ghost-Write tx
//! ```
//!
//! ## Q-Manifest Law Compliance
//! - **Law 2 (Immutable Binaries)**: history is read-only — cannot modify past versions
//! - **Law 5 (Global Deduplication)**: if two adjacent versions have the same OID,
//!   no new Shadow Object was created (dedup already prevented the write)
//! - **Law 9 (Universal Namespace)**: each version addressed as `prism://<oid>`

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Version Entry ─────────────────────────────────────────────────────────────

/// A single version in a Timeline.
#[derive(Debug, Clone)]
pub struct VersionEntry {
    /// OID of this version
    pub oid: [u8; 32],
    /// Version number (1 = original, 2 = first edit, etc.)
    pub version: u32,
    /// Kernel tick when this version was created
    pub created_at: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Size delta vs previous version (positive = grew, negative = shrunk)
    pub size_delta: i64,
    /// Tick delta since previous version
    pub tick_delta: u64,
    /// Human-readable label (auto-generated or user-tagged)
    pub label: String,
    /// Was this version manually tagged by the user?
    pub user_tagged: bool,
    /// UNS address of this version: `prism://sha256:<oid_hex>`
    pub uns_uri: String,
}

impl VersionEntry {
    pub fn oid_hex_short(&self) -> String {
        let mut s = String::new();
        for b in &self.oid[..4] {
            let hi = b >> 4;
            let lo = b & 0xF;
            s.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + hi - 10) as char });
            s.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + lo - 10) as char });
        }
        s.push_str("..");
        s
    }
}

// ── Timeline ──────────────────────────────────────────────────────────────────

/// A complete version history timeline for one Prism object.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// The current (latest) OID
    pub current_oid: [u8; 32],
    /// Object type label
    pub object_type: String,
    /// Ordered versions: index 0 = oldest
    pub versions: Vec<VersionEntry>,
    /// Is this timeline currently held open? (ref prevents Shadow GC)
    pub pinned: bool,
    /// Which version is "previewed" in Aether (None = latest)
    pub preview_version: Option<u32>,
}

impl Timeline {
    pub fn len(&self) -> usize { self.versions.len() }

    pub fn current_version(&self) -> Option<&VersionEntry> {
        self.versions.last()
    }

    /// Get a version by 1-based version number.
    pub fn get_version(&self, version: u32) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Time span covered by this timeline (ticks).
    pub fn span_ticks(&self) -> u64 {
        match (self.versions.first(), self.versions.last()) {
            (Some(f), Some(l)) => l.created_at.saturating_sub(f.created_at),
            _ => 0,
        }
    }

    /// Set the preview position (Aether slider drag).
    pub fn set_preview(&mut self, version: u32) -> bool {
        if self.get_version(version).is_none() { return false; }
        self.preview_version = Some(version);
        true
    }

    pub fn clear_preview(&mut self) {
        self.preview_version = None;
    }
}

// ── Restore Plan ──────────────────────────────────────────────────────────────

/// A plan to restore a previous version (becomes a Ghost-Write transaction).
#[derive(Debug, Clone)]
pub struct RestorePlan {
    /// Object to restore
    pub object_oid: [u8; 32],
    /// Version to restore to
    pub target_version: u32,
    /// The OID that will become the source for the restore write
    pub source_oid: [u8; 32],
    /// Estimated byte size of the restore write
    pub size_bytes: u64,
    /// A new Ghost-Write transaction should be opened with this
    pub description: String,
}

// ── Timeline Statistics ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TimelineStats {
    pub timelines_built: u64,
    pub versions_tracked: u64,
    pub restores_executed: u64,
    pub previews_set: u64,
    pub timelines_pinned: u64,
}

// ── Timeline Navigator ────────────────────────────────────────────────────────

/// The ghost-write version history navigator.
pub struct TimelineNavigator {
    /// Cached timelines: current_oid key → Timeline
    pub timelines: BTreeMap<u64, Timeline>,
    /// Statistics
    pub stats: TimelineStats,
}

impl TimelineNavigator {
    pub fn new() -> Self {
        TimelineNavigator {
            timelines: BTreeMap::new(),
            stats: TimelineStats::default(),
        }
    }

    fn oid_key(oid: &[u8; 32]) -> u64 {
        u64::from_le_bytes([oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7]])
    }

    /// Build a Timeline by walking the Shadow Object chain from the current OID.
    /// `shadow_lookup`: a closure/fn that looks up a ShadowObject by OID.
    /// Returns the constructed Timeline.
    pub fn build_timeline(
        &mut self,
        current_oid: [u8; 32],
        current_size: u64,
        current_tick: u64,
        object_type: &str,
        shadow_chain: &[([u8; 32], u64, u64)], // (oid, size_bytes, created_at) oldest→newer
    ) -> &Timeline {
        let key = Self::oid_key(&current_oid);

        let mut versions: Vec<VersionEntry> = Vec::new();
        let mut prev_size = 0u64;
        let mut prev_tick = 0u64;
        let mut version_num = 1u32;

        // Add historical versions (oldest first)
        for &(oid, size, tick) in shadow_chain.iter() {
            let uns_hex: String = {
                let mut s = "prism://sha256:".to_string();
                for b in &oid[..8] {
                    let hi = b >> 4;
                    let lo = b & 0xF;
                    s.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + hi - 10) as char });
                    s.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + lo - 10) as char });
                }
                s.push_str("..");
                s
            };
            versions.push(VersionEntry {
                oid,
                version: version_num,
                created_at: tick,
                size_bytes: size,
                size_delta: if version_num == 1 { 0 } else { size as i64 - prev_size as i64 },
                tick_delta: if version_num == 1 { 0 } else { tick - prev_tick },
                label: alloc::format!("v{}", version_num),
                user_tagged: false,
                uns_uri: uns_hex,
            });
            prev_size = size;
            prev_tick = tick;
            version_num += 1;
        }

        // Add current version
        let uns_hex_cur: String = {
            let mut s = "prism://sha256:".to_string();
            for b in &current_oid[..8] {
                let hi = b >> 4;
                let lo = b & 0xF;
                s.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + hi - 10) as char });
                s.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + lo - 10) as char });
            }
            s.push_str("..");
            s
        };
        versions.push(VersionEntry {
            oid: current_oid,
            version: version_num,
            created_at: current_tick,
            size_bytes: current_size,
            size_delta: current_size as i64 - prev_size as i64,
            tick_delta: current_tick - prev_tick,
            label: alloc::format!("v{} (current)", version_num),
            user_tagged: false,
            uns_uri: uns_hex_cur,
        });

        self.stats.versions_tracked += version_num as u64;
        self.stats.timelines_built += 1;

        crate::serial_println!(
            "[TIMELINE] Built timeline for {:02x}{:02x}...: {} versions spanning {} ticks",
            current_oid[0], current_oid[1], version_num,
            versions.last().map(|v| v.created_at).unwrap_or(0)
                .saturating_sub(versions.first().map(|v| v.created_at).unwrap_or(0))
        );

        let timeline = Timeline {
            current_oid,
            object_type: object_type.to_string(),
            versions,
            pinned: false,
            preview_version: None,
        };

        self.timelines.insert(key, timeline);
        self.timelines.get(&key).unwrap()
    }

    /// Get a previously built timeline.
    pub fn get_timeline(&self, current_oid: &[u8; 32]) -> Option<&Timeline> {
        self.timelines.get(&Self::oid_key(current_oid))
    }

    /// Set preview position (Aether slider drag event).
    pub fn set_preview(&mut self, current_oid: &[u8; 32], version: u32) -> bool {
        let key = Self::oid_key(current_oid);
        if let Some(t) = self.timelines.get_mut(&key) {
            let ok = t.set_preview(version);
            if ok {
                self.stats.previews_set += 1;
                crate::serial_println!(
                    "[TIMELINE] Preview → v{} of {:02x}{:02x}..",
                    version, current_oid[0], current_oid[1]
                );
            }
            ok
        } else { false }
    }

    /// Build a RestorePlan for a given version (caller runs the Ghost-Write commit).
    pub fn plan_restore(&self, current_oid: &[u8; 32], target_version: u32) -> Option<RestorePlan> {
        let timeline = self.get_timeline(current_oid)?;
        let entry = timeline.get_version(target_version)?;
        crate::serial_println!(
            "[TIMELINE] Restore plan: {:02x}{:02x}.. v{} → v{} ({} bytes)",
            current_oid[0], current_oid[1],
            timeline.versions.len(), target_version, entry.size_bytes
        );
        Some(RestorePlan {
            object_oid: *current_oid,
            target_version,
            source_oid: entry.oid,
            size_bytes: entry.size_bytes,
            description: alloc::format!(
                "Restore {} to {} ({}B)",
                timeline.object_type, entry.label, entry.size_bytes
            ),
        })
    }

    /// Mark a restore as completed (increments stat counter).
    pub fn mark_restored(&mut self) {
        self.stats.restores_executed += 1;
    }

    /// User-tag a version with a label ("Before big edit", "Release 1.0", etc.).
    pub fn tag_version(
        &mut self,
        current_oid: &[u8; 32],
        version: u32,
        label: &str,
    ) -> bool {
        let key = Self::oid_key(current_oid);
        if let Some(t) = self.timelines.get_mut(&key) {
            if let Some(entry) = t.versions.iter_mut().find(|v| v.version == version) {
                entry.label = label.to_string();
                entry.user_tagged = true;
                crate::serial_println!(
                    "[TIMELINE] Tagged v{} of {:02x}{:02x}.. as \"{}\"",
                    version, current_oid[0], current_oid[1], label
                );
                return true;
            }
        }
        false
    }

    /// Pin a timeline so Shadow Objects are not garbage-collected.
    pub fn pin(&mut self, current_oid: &[u8; 32]) {
        let key = Self::oid_key(current_oid);
        if let Some(t) = self.timelines.get_mut(&key) {
            t.pinned = true;
            self.stats.timelines_pinned += 1;
        }
    }

    /// Evict a timeline from the navigator cache (Shadow Object refs released).
    pub fn evict(&mut self, current_oid: &[u8; 32]) -> bool {
        self.timelines.remove(&Self::oid_key(current_oid)).is_some()
    }
}
