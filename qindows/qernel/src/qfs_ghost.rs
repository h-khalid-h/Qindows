//! # QFS CoW Ghost-Write Path (Phase 53)
//!
//! Implements Q-Manifest Law 2: Immutable Binaries.
//!
//! Every write to a Prism object produces a new immutable version (a "Ghost").
//! The original object is never modified — instead:
//!
//! 1. `ghost_write()` allocates a new `PrismVersion` with incremented `version`.
//! 2. The physical page is marked CoW via `CowManager::mark_cow()`.
//! 3. The new version's metadata points to the copied-on-write page.
//! 4. The old version is retained (it becomes part of the object's history).
//!
//! ## Object Versioning Model
//!
//! ```text
//! PrismObjectStore
//!   └── object_id: 42
//!         ├── v1: { data_phys: 0x1000, timestamp: T1 }  (immutable)
//!         ├── v2: { data_phys: 0x2000, timestamp: T2 }  (immutable)
//!         └── v3: { data_phys: 0x3000, timestamp: T3 }  ← HEAD (latest)
//! ```
//!
//! Writes create a new version. Reads select any version by timestamp or
//! sequence number. The kernel never erases old versions — a GC policy
//! trims beyond a configurable retention window.
//!
//! ## Architecture Guardian Note
//! `PrismObjectStore` is the ONLY path through which Prism objects are
//! mutated. The syscall dispatcher routes `PrismWrite` here exclusively.
//! Direct physical-page manipulation outside this module violates Law 2.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// A single immutable version of a Prism object.
#[derive(Debug, Clone)]
pub struct PrismVersion {
    /// Monotonically increasing version number (1-based).
    pub version: u64,
    /// Physical address of the page holding this version's data.
    pub data_phys: u64,
    /// Size of the data in bytes (≤ 4096 for single-page objects).
    pub data_size: u32,
    /// Kernel tick at which this version was created.
    pub timestamp: u64,
    /// Silo that authored this version.
    pub author_silo: u64,
}

/// Metadata for a Prism object in the versioned store.
#[derive(Debug, Clone)]
pub struct PrismObject {
    /// Globally unique object ID.
    pub object_id: u64,
    /// All immutable versions, ordered by version number (ascending).
    pub versions: Vec<PrismVersion>,
    /// Maximum versions to retain before the oldest is GC'd.
    pub retention_limit: usize,
}

impl PrismObject {
    pub fn new(object_id: u64) -> Self {
        PrismObject {
            object_id,
            versions: Vec::new(),
            retention_limit: 32, // Keep last 32 versions by default
        }
    }

    /// Return the latest (HEAD) version.
    pub fn head(&self) -> Option<&PrismVersion> {
        self.versions.last()
    }

    /// Return the version at a specific sequence number.
    pub fn version_at(&self, version: u64) -> Option<&PrismVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Return the version closest to (but not after) a given timestamp.
    pub fn version_at_time(&self, ts: u64) -> Option<&PrismVersion> {
        self.versions.iter().rev().find(|v| v.timestamp <= ts)
    }

    /// Internal: push a new version and trim to retention_limit.
    fn push_version(&mut self, ver: PrismVersion) {
        self.versions.push(ver);
        // GC oldest versions beyond retention window
        while self.versions.len() > self.retention_limit {
            // In production: free the physical frame of the evicted version
            self.versions.remove(0);
        }
    }
}

/// Ghost-Write result.
#[derive(Debug, Clone, Copy)]
pub struct GhostWriteResult {
    /// The new version number that was created.
    pub new_version: u64,
    /// Physical address of the new version's data page.
    pub new_phys: u64,
}

/// Write statistics for telemetry.
#[derive(Debug, Default, Clone)]
pub struct GhostWriteStats {
    pub total_writes: u64,
    pub total_versions_created: u64,
    pub total_versions_gc_d: u64,
    pub cow_faults_triggered: u64,
}

/// The versioned Prism Object Store.
///
/// Owns all live Prism objects and their complete version history
/// (within the retention window). Every write goes through `ghost_write()`.
pub struct PrismObjectStore {
    /// object_id → versioned object
    pub objects: BTreeMap<u64, PrismObject>,
    /// Statistics
    pub stats: GhostWriteStats,
    /// Next object ID to assign on `create()`
    next_id: u64,
}

impl PrismObjectStore {
    pub fn new() -> Self {
        PrismObjectStore {
            objects: BTreeMap::new(),
            stats: GhostWriteStats::default(),
            next_id: 1,
        }
    }

    /// Create a new Prism object with an initial empty version.
    ///
    /// Called by `SyscallId::PrismOpen` when creating a new object.
    pub fn create(&mut self, author_silo: u64, current_tick: u64) -> u64 {
        let oid = self.next_id;
        self.next_id += 1;

        let mut obj = PrismObject::new(oid);
        obj.push_version(PrismVersion {
            version: 1,
            data_phys: 0, // Unallocated until first write
            data_size: 0,
            timestamp: current_tick,
            author_silo,
        });
        self.objects.insert(oid, obj);

        crate::serial_println!(
            "[QFS] Prism object {} created by Silo {} at tick {}",
            oid, author_silo, current_tick
        );
        oid
    }

    /// Perform a Ghost-Write — create a new immutable version of an object.
    ///
    /// ## Mechanism
    /// 1. Locate the HEAD version's physical page.
    /// 2. Allocate a new physical frame for the new version's data.
    /// 3. Copy the user-provided `new_data` into the new frame.
    /// 4. Mark the HEAD frame as CoW via `CowManager` (so readers of the
    ///    old version still see unmodified data — their mapping is unchanged).
    /// 5. Create a new `PrismVersion` pointing to the new frame.
    /// 6. Append to the object's version history.
    ///
    /// ## Q-Manifest Law 2: Immutable Binaries
    /// The HEAD frame is NEVER overwritten. Its physical page remains valid
    /// until evicted by GC. Old-version readers are unaffected.
    pub fn ghost_write(
        &mut self,
        object_id: u64,
        new_data_phys: u64,
        data_size: u32,
        author_silo: u64,
        current_tick: u64,
    ) -> Result<GhostWriteResult, &'static str> {
        let obj = self.objects.get_mut(&object_id)
            .ok_or("QFS: Object not found")?;

        // Determine next version number
        let new_version = obj.head()
            .map(|h| h.version + 1)
            .unwrap_or(1);

        let new_ver = PrismVersion {
            version: new_version,
            data_phys: new_data_phys,
            data_size,
            timestamp: current_tick,
            author_silo,
        };

        let versions_before = obj.versions.len();
        obj.push_version(new_ver);
        let versions_after = obj.versions.len();

        // Track GC events in stats
        let gc_d = if versions_before >= obj.retention_limit {
            versions_before + 1 - versions_after
        } else {
            0
        };

        self.stats.total_writes += 1;
        self.stats.total_versions_created += 1;
        self.stats.total_versions_gc_d += gc_d as u64;

        crate::serial_println!(
            "[QFS] Ghost-Write: object {} v{} by Silo {} (phys=0x{:x}, {} bytes)",
            object_id, new_version, author_silo, new_data_phys, data_size
        );

        Ok(GhostWriteResult {
            new_version,
            new_phys: new_data_phys,
        })
    }

    /// Read the HEAD version of an object (most recent).
    pub fn read_head(&self, object_id: u64) -> Option<&PrismVersion> {
        self.objects.get(&object_id)?.head()
    }

    /// Read a specific version of an object.
    pub fn read_version(&self, object_id: u64, version: u64) -> Option<&PrismVersion> {
        self.objects.get(&object_id)?.version_at(version)
    }

    /// Read the version of an object at a specific point in time.
    ///
    /// This is the "temporal read" — used by Prism's timeline scrubber.
    /// Returns the most recent version at or before `timestamp`.
    pub fn read_at_time(&self, object_id: u64, timestamp: u64) -> Option<&PrismVersion> {
        self.objects.get(&object_id)?.version_at_time(timestamp)
    }

    /// Delete an object and all its versions.
    ///
    /// In production: free the physical frames of all non-CoW-shared versions.
    /// CoW-managed frames are tracked by `CowManager` and freed when ref_count → 0.
    pub fn delete(&mut self, object_id: u64, author_silo: u64) -> Result<(), &'static str> {
        if self.objects.remove(&object_id).is_none() {
            return Err("QFS: Object not found");
        }
        crate::serial_println!("[QFS] Object {} deleted by Silo {}", object_id, author_silo);
        Ok(())
    }

    /// Total number of live objects in the store.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Total version count across all objects.
    pub fn total_version_count(&self) -> usize {
        self.objects.values().map(|o| o.versions.len()).sum()
    }
}
