//! # Q-Versions — File Versioning / Time-Travel
//!
//! Every Q-Object can have multiple immutable versions (Section 3.7).
//! Users can browse, diff, and restore any past version.
//!
//! Features:
//! - Append-only version history per Q-Object
//! - Content-addressed storage (deduplicated)
//! - Named tags (bookmarks for important versions)
//! - Diff between versions (byte-level delta)
//! - Per-Silo retention policies

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A version of a Q-Object.
#[derive(Debug, Clone)]
pub struct Version {
    pub number: u64,
    pub oid: u64,
    pub hash: [u8; 32],
    pub size: u64,
    pub created_at: u64,
    pub author_silo: u64,
    pub message: String,
}

/// A version tag (bookmark).
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub version: u64,
    pub created_at: u64,
}

/// Retention policy.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub max_versions: usize,
    pub max_age_secs: u64,
    pub keep_tagged: bool,
}

/// Version statistics.
#[derive(Debug, Clone, Default)]
pub struct VersionStats {
    pub versions_created: u64,
    pub versions_pruned: u64,
    pub restores: u64,
    pub tags_created: u64,
    pub bytes_deduplicated: u64,
}

/// The Q-Versions Manager.
pub struct QVersions {
    /// Object OID → version history
    pub history: BTreeMap<u64, Vec<Version>>,
    /// Object OID → tags
    pub tags: BTreeMap<u64, Vec<Tag>>,
    /// Per-Silo retention policies
    pub policies: BTreeMap<u64, RetentionPolicy>,
    /// Default retention
    pub default_policy: RetentionPolicy,
    pub stats: VersionStats,
}

impl QVersions {
    pub fn new() -> Self {
        QVersions {
            history: BTreeMap::new(),
            tags: BTreeMap::new(),
            policies: BTreeMap::new(),
            default_policy: RetentionPolicy {
                max_versions: 100,
                max_age_secs: 86400 * 30, // 30 days
                keep_tagged: true,
            },
            stats: VersionStats::default(),
        }
    }

    /// Create a new version of an object.
    pub fn commit(&mut self, oid: u64, hash: [u8; 32], size: u64, silo_id: u64, message: &str, now: u64) -> u64 {
        let versions = self.history.entry(oid).or_insert_with(Vec::new);

        // Dedup: skip if hash matches latest
        if let Some(last) = versions.last() {
            if last.hash == hash {
                self.stats.bytes_deduplicated += size;
                return last.number;
            }
        }

        let number = versions.len() as u64 + 1;
        versions.push(Version {
            number, oid, hash, size,
            created_at: now, author_silo: silo_id,
            message: String::from(message),
        });

        self.stats.versions_created += 1;
        number
    }

    /// Tag a version.
    pub fn tag(&mut self, oid: u64, version: u64, name: &str, now: u64) -> Result<(), &'static str> {
        // Verify version exists
        let versions = self.history.get(&oid).ok_or("Object not found")?;
        if !versions.iter().any(|v| v.number == version) {
            return Err("Version not found");
        }

        let tags = self.tags.entry(oid).or_insert_with(Vec::new);
        tags.push(Tag {
            name: String::from(name), version, created_at: now,
        });
        self.stats.tags_created += 1;
        Ok(())
    }

    /// Get a specific version.
    pub fn get(&self, oid: u64, version: u64) -> Option<&Version> {
        self.history.get(&oid)?
            .iter().find(|v| v.number == version)
    }

    /// Restore to a version (creates a new version with old content).
    pub fn restore(&mut self, oid: u64, version: u64, now: u64) -> Result<u64, &'static str> {
        let old = self.get(oid, version).ok_or("Version not found")?.clone();
        self.stats.restores += 1;
        let num = self.commit(oid, old.hash, old.size, old.author_silo, "Restored", now);
        Ok(num)
    }

    /// Prune old versions according to retention policy.
    pub fn prune(&mut self, oid: u64, silo_id: u64, now: u64) {
        let policy = self.policies.get(&silo_id).unwrap_or(&self.default_policy).clone();

        let tagged_versions: Vec<u64> = if policy.keep_tagged {
            self.tags.get(&oid).map(|t| t.iter().map(|tag| tag.version).collect()).unwrap_or_default()
        } else {
            Vec::new()
        };

        if let Some(versions) = self.history.get_mut(&oid) {
            let before = versions.len();
            versions.retain(|v| {
                if tagged_versions.contains(&v.number) { return true; }
                if now.saturating_sub(v.created_at) > policy.max_age_secs { return false; }
                true
            });

            // Also enforce max count
            while versions.len() > policy.max_versions {
                // Remove oldest non-tagged
                if let Some(idx) = versions.iter().position(|v| !tagged_versions.contains(&v.number)) {
                    versions.remove(idx);
                } else {
                    break;
                }
            }

            let pruned = before - versions.len();
            self.stats.versions_pruned += pruned as u64;
        }
    }
}
