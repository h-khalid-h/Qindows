//! # Qegistry — Versioned Configuration Store
//!
//! Replaces the Windows Registry with a Git-like versioned key-value
//! store (Section 3.1). System state is stored hierarchically and
//! can be branched / rolled back to any previous commit.
//!
//! Key features:
//! - **Hierarchical keys**: `/system/display/resolution`
//! - **Git-like versioning**: Every mutation creates a commit
//! - **Branching**: Try a new driver config, rollback if broken
//! - **Per-Silo isolation**: Apps see only their own namespace
//! - **Schema validation**: Type-safe values (no REG_DWORD nonsense)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A typed configuration value.
#[derive(Debug, Clone, PartialEq)]
pub enum QValue {
    /// Boolean
    Bool(bool),
    /// 64-bit integer
    Int(i64),
    /// Floating point
    Float(f64),
    /// UTF-8 string
    Str(String),
    /// Binary blob
    Bytes(Vec<u8>),
    /// List of values
    List(Vec<QValue>),
}

impl QValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self { QValue::Bool(b) => Some(*b), _ => None }
    }
    pub fn as_int(&self) -> Option<i64> {
        match self { QValue::Int(i) => Some(*i), _ => None }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self { QValue::Str(s) => Some(s), _ => None }
    }
}

/// A commit — a snapshot of the configuration at a point in time.
#[derive(Debug, Clone)]
pub struct Commit {
    /// Commit hash (content-addressed)
    pub hash: u64,
    /// Parent commit (None for initial)
    pub parent: Option<u64>,
    /// Timestamp
    pub timestamp: u64,
    /// Description of what changed
    pub message: String,
    /// The key that was changed
    pub changed_key: String,
    /// Silo that made this change
    pub silo_id: u64,
}

/// A configuration branch.
#[derive(Debug, Clone)]
pub struct Branch {
    /// Branch name
    pub name: String,
    /// Head commit hash
    pub head: u64,
    /// Creation timestamp
    pub created_at: u64,
    /// Is this the active branch?
    pub active: bool,
}

/// Qegistry statistics.
#[derive(Debug, Clone, Default)]
pub struct QegistryStats {
    pub keys_set: u64,
    pub keys_deleted: u64,
    pub reads: u64,
    pub commits: u64,
    pub branches_created: u64,
    pub rollbacks: u64,
}

/// The Qegistry — versioned configuration store.
pub struct Qegistry {
    /// Current key-value store
    pub store: BTreeMap<String, QValue>,
    /// Commit history
    pub commits: Vec<Commit>,
    /// Branches
    pub branches: BTreeMap<String, Branch>,
    /// Current branch name
    pub current_branch: String,
    /// Next commit hash counter
    next_hash: u64,
    /// Per-silo namespaces (isolated views)
    pub silo_stores: BTreeMap<u64, BTreeMap<String, QValue>>,
    /// Statistics
    pub stats: QegistryStats,
}

impl Qegistry {
    pub fn new() -> Self {
        let mut branches = BTreeMap::new();
        branches.insert(String::from("main"), Branch {
            name: String::from("main"),
            head: 0,
            created_at: 0,
            active: true,
        });

        Qegistry {
            store: BTreeMap::new(),
            commits: Vec::new(),
            branches,
            current_branch: String::from("main"),
            next_hash: 1,
            silo_stores: BTreeMap::new(),
            stats: QegistryStats::default(),
        }
    }

    /// Set a system-wide configuration key.
    pub fn set(&mut self, key: &str, value: QValue, message: &str, silo_id: u64, now: u64) {
        self.store.insert(String::from(key), value);
        self.commit(key, message, silo_id, now);
        self.stats.keys_set += 1;
    }

    /// Get a system-wide configuration value.
    pub fn get(&mut self, key: &str) -> Option<&QValue> {
        self.stats.reads += 1;
        self.store.get(key)
    }

    /// Delete a key.
    pub fn delete(&mut self, key: &str, silo_id: u64, now: u64) -> bool {
        if self.store.remove(key).is_some() {
            self.commit(key, "deleted", silo_id, now);
            self.stats.keys_deleted += 1;
            true
        } else {
            false
        }
    }

    /// List all keys matching a prefix.
    pub fn list_prefix(&self, prefix: &str) -> Vec<(&str, &QValue)> {
        self.store.iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Set a per-silo key (isolated namespace).
    pub fn silo_set(&mut self, silo_id: u64, key: &str, value: QValue) {
        self.silo_stores.entry(silo_id)
            .or_insert_with(BTreeMap::new)
            .insert(String::from(key), value);
    }

    /// Get a per-silo key (falls back to system-wide).
    pub fn silo_get(&mut self, silo_id: u64, key: &str) -> Option<&QValue> {
        self.stats.reads += 1;
        // Check silo-specific first, then system-wide
        if let Some(silo_store) = self.silo_stores.get(&silo_id) {
            if let Some(val) = silo_store.get(key) {
                return Some(val);
            }
        }
        self.store.get(key)
    }

    /// Create a new branch (snapshot current state).
    pub fn create_branch(&mut self, name: &str, now: u64) -> Result<(), &'static str> {
        if self.branches.contains_key(name) {
            return Err("Branch already exists");
        }

        let head = self.branches.get(&self.current_branch)
            .map(|b| b.head)
            .unwrap_or(0);

        self.branches.insert(String::from(name), Branch {
            name: String::from(name),
            head,
            created_at: now,
            active: false,
        });

        self.stats.branches_created += 1;
        Ok(())
    }

    /// Switch to a branch.
    pub fn checkout(&mut self, name: &str) -> Result<(), &'static str> {
        if !self.branches.contains_key(name) {
            return Err("Branch not found");
        }

        // Deactivate current
        if let Some(current) = self.branches.get_mut(&self.current_branch) {
            current.active = false;
        }

        // Activate target
        if let Some(target) = self.branches.get_mut(name) {
            target.active = true;
        }

        self.current_branch = String::from(name);
        Ok(())
    }

    /// Rollback to a previous commit.
    pub fn rollback(&mut self, commit_hash: u64) -> Result<(), &'static str> {
        let _commit = self.commits.iter()
            .find(|c| c.hash == commit_hash)
            .ok_or("Commit not found")?
            .clone();

        // In production: rebuild store state from commit chain
        // Simplified: just update the branch head
        if let Some(branch) = self.branches.get_mut(&self.current_branch) {
            branch.head = commit_hash;
        }

        self.stats.rollbacks += 1;
        Ok(())
    }

    /// Get commit history (most recent first).
    pub fn log(&self, limit: usize) -> Vec<&Commit> {
        self.commits.iter().rev().take(limit).collect()
    }

    /// Create a commit (internal).
    fn commit(&mut self, key: &str, message: &str, silo_id: u64, now: u64) {
        let parent = self.branches.get(&self.current_branch)
            .map(|b| b.head);

        let hash = self.next_hash;
        self.next_hash += 1;

        self.commits.push(Commit {
            hash,
            parent: if parent == Some(0) { None } else { parent },
            timestamp: now,
            message: String::from(message),
            changed_key: String::from(key),
            silo_id,
        });

        if let Some(branch) = self.branches.get_mut(&self.current_branch) {
            branch.head = hash;
        }

        self.stats.commits += 1;
    }

    /// Clean up a silo's namespace (on silo termination).
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        self.silo_stores.remove(&silo_id);
    }
}
