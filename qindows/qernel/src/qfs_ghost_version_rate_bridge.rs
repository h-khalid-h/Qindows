//! # QFS Ghost Version Rate Bridge (Phase 283)
//!
//! ## Architecture Guardian: The Gap
//! `qfs_ghost.rs` implements `PrismObject`:
//! - `PrismObject { object_id, versions: Vec<PrismVersion> }`
//! - `PrismObject::version_at(version)` → Option<&PrismVersion>
//! - `PrismObject::version_at_time(ts)` → Option<&PrismVersion>
//!
//! **Missing link**: PrismObject version history was unbounded. A
//! frequently-written file could accumulate thousands of versions,
//! causing `version_at_time()` to O(n) scan large version arrays.
//!
//! This module provides `QFsGhostVersionRateBridge`:
//! Max 256 versions per PrismObject (Ghost FS version history cap).

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_VERSIONS_PER_OBJECT: u64 = 256;

#[derive(Debug, Default, Clone)]
pub struct QFsGhostVersionStats {
    pub versions_allowed: u64,
    pub versions_denied:  u64,
}

pub struct QFsGhostVersionRateBridge {
    object_version_counts: BTreeMap<u64, u64>, // object_id → version count
    pub stats:             QFsGhostVersionStats,
}

impl QFsGhostVersionRateBridge {
    pub fn new() -> Self {
        QFsGhostVersionRateBridge { object_version_counts: BTreeMap::new(), stats: QFsGhostVersionStats::default() }
    }

    pub fn allow_version_create(&mut self, object_id: u64) -> bool {
        let count = self.object_version_counts.entry(object_id).or_default();
        if *count >= MAX_VERSIONS_PER_OBJECT {
            self.stats.versions_denied += 1;
            crate::serial_println!(
                "[QFS GHOST] Object {} version cap reached ({}/{})", object_id, count, MAX_VERSIONS_PER_OBJECT
            );
            return false;
        }
        *count += 1;
        self.stats.versions_allowed += 1;
        true
    }

    pub fn on_object_delete(&mut self, object_id: u64) {
        self.object_version_counts.remove(&object_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QFsGhostVersionBridge: allowed={} denied={}",
            self.stats.versions_allowed, self.stats.versions_denied
        );
    }
}
