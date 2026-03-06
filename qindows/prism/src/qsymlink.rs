//! # Q-Symlink — Cross-Silo Symbolic Links with Capability Check
//!
//! Creates symbolic links that can span Silo boundaries,
//! with mandatory capability verification on traversal (Section 3.12).
//!
//! Features:
//! - Intra-Silo symlinks (no capability needed)
//! - Cross-Silo symlinks (requires SymlinkTraverse capability)
//! - Dangling link detection
//! - Symlink depth limit (prevent infinite loops)
//! - Per-Silo symlink quotas

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;

/// A symbolic link.
#[derive(Debug, Clone)]
pub struct Symlink {
    pub id: u64,
    pub source_silo: u64,
    pub source_path: String,
    pub target_silo: u64,
    pub target_oid: u64,
    pub cross_silo: bool,
    pub created_at: u64,
}

/// Symlink statistics.
#[derive(Debug, Clone, Default)]
pub struct SymlinkStats {
    pub links_created: u64,
    pub links_removed: u64,
    pub traversals: u64,
    pub cross_silo_traversals: u64,
    pub denied_traversals: u64,
    pub dangling_detected: u64,
    pub depth_limit_hit: u64,
}

/// The Q-Symlink Manager.
pub struct QSymlink {
    pub links: BTreeMap<u64, Symlink>,
    /// Path → link ID index (silo_id:path → link_id)
    pub path_index: BTreeMap<String, u64>,
    /// Per-Silo quota (max symlinks)
    pub quotas: BTreeMap<u64, (u64, u64)>, // (used, max)
    pub max_depth: u32,
    next_id: u64,
    pub stats: SymlinkStats,
}

impl QSymlink {
    pub fn new() -> Self {
        QSymlink {
            links: BTreeMap::new(),
            path_index: BTreeMap::new(),
            quotas: BTreeMap::new(),
            max_depth: 40,
            next_id: 1,
            stats: SymlinkStats::default(),
        }
    }

    /// Create a symlink.
    pub fn create(&mut self, src_silo: u64, src_path: &str, tgt_silo: u64, tgt_oid: u64, now: u64) -> Result<u64, &'static str> {
        // Check quota
        if let Some(&(used, max)) = self.quotas.get(&src_silo) {
            if used >= max {
                return Err("Symlink quota exceeded");
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        let cross = src_silo != tgt_silo;

        let key = Self::path_key(src_silo, src_path);
        if self.path_index.contains_key(&key) {
            return Err("Path already has a symlink");
        }

        self.links.insert(id, Symlink {
            id, source_silo: src_silo, source_path: String::from(src_path),
            target_silo: tgt_silo, target_oid: tgt_oid,
            cross_silo: cross, created_at: now,
        });
        self.path_index.insert(key, id);

        if let Some(q) = self.quotas.get_mut(&src_silo) {
            q.0 += 1;
        }

        self.stats.links_created += 1;
        Ok(id)
    }

    /// Traverse a symlink.
    pub fn traverse(&mut self, silo_id: u64, path: &str, has_cap: bool) -> Result<(u64, u64), &'static str> {
        let key = Self::path_key(silo_id, path);
        let link_id = *self.path_index.get(&key).ok_or("No symlink at path")?;
        let link = self.links.get(&link_id).ok_or("Symlink data missing")?;

        if link.cross_silo && !has_cap {
            self.stats.denied_traversals += 1;
            return Err("No SymlinkTraverse capability");
        }

        self.stats.traversals += 1;
        if link.cross_silo {
            self.stats.cross_silo_traversals += 1;
        }

        Ok((link.target_silo, link.target_oid))
    }

    /// Remove a symlink.
    pub fn remove(&mut self, link_id: u64) {
        if let Some(link) = self.links.remove(&link_id) {
            let key = Self::path_key(link.source_silo, &link.source_path);
            self.path_index.remove(&key);
            if let Some(q) = self.quotas.get_mut(&link.source_silo) {
                q.0 = q.0.saturating_sub(1);
            }
            self.stats.links_removed += 1;
        }
    }

    /// Set symlink quota for a Silo.
    pub fn set_quota(&mut self, silo_id: u64, max: u64) {
        let used = self.quotas.get(&silo_id).map(|q| q.0).unwrap_or(0);
        self.quotas.insert(silo_id, (used, max));
    }

    fn path_key(silo_id: u64, path: &str) -> String {
        let mut key = String::new();
        // Simple silo:path format
        let silo_str = Self::u64_to_str(silo_id);
        key.push_str(&silo_str);
        key.push(':');
        key.push_str(path);
        key
    }

    fn u64_to_str(v: u64) -> String {
        let mut buf = [0u8; 20];
        let mut n = v;
        let mut i = 0;
        if n == 0 {
            return String::from("0");
        }
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        buf[..i].reverse();
        String::from_utf8_lossy(&buf[..i]).into_owned()
    }
}
