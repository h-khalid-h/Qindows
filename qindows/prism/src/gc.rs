//! # Prism Garbage Collector
//!
//! Reclaims orphaned objects in the Prism object graph.
//! An object is "orphaned" when its reference count drops to zero,
//! meaning no Silo, snapshot, or dedup index points to it.
//!
//! The GC runs in background Fibers during idle time, using a
//! tri-color mark-and-sweep algorithm on the object graph:
//!
//! 1. **Mark (White→Gray)**: Start from root set (active snapshots,
//!    open handles, pinned objects). Mark all reachable objects gray.
//! 2. **Trace (Gray→Black)**: For each gray object, trace its
//!    references; mark children gray, self black.
//! 3. **Sweep (White→Free)**: All remaining white objects are
//!    unreachable — reclaim their storage.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// Object ID (matches Prism OID).
pub type Oid = u64;

/// Block address on disk.
pub type BlockAddr = u64;

/// GC color for tri-color marking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcColor {
    /// Not yet visited (candidate for collection)
    White,
    /// Discovered but children not yet traced
    Gray,
    /// Fully traced (reachable, keep)
    Black,
}

/// A tracked object in the GC's view.
#[derive(Debug, Clone)]
pub struct GcObject {
    /// Object ID
    pub oid: Oid,
    /// Color for this GC cycle
    pub color: GcColor,
    /// Block address on disk
    pub block_addr: BlockAddr,
    /// Size in bytes
    pub size: u64,
    /// References to other objects (edges in the object graph)
    pub references: Vec<Oid>,
    /// External reference count (open handles, snapshot pins)
    pub external_refs: u32,
    /// Last access timestamp (ticks)
    pub last_access: u64,
    /// Generation (number of GC cycles survived)
    pub generation: u32,
}

/// GC phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcPhase {
    /// Not running
    Idle,
    /// Building root set
    RootDiscovery,
    /// Mark phase (tracing reachable objects)
    Marking,
    /// Sweep phase (reclaiming unreachable objects)
    Sweeping,
    /// Compaction (optional — defragments storage)
    Compacting,
}

/// GC statistics.
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total GC cycles run
    pub cycles: u64,
    /// Total objects collected (freed)
    pub objects_collected: u64,
    /// Total bytes reclaimed
    pub bytes_reclaimed: u64,
    /// Total objects currently tracked
    pub objects_tracked: u64,
    /// Objects marked as reachable in last cycle
    pub last_reachable: u64,
    /// Objects collected in last cycle
    pub last_collected: u64,
    /// Duration of last cycle (ticks)
    pub last_duration: u64,
}

/// GC configuration.
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Trigger GC when tracked objects exceed this count
    pub object_threshold: u64,
    /// Trigger GC when total tracked bytes exceed this
    pub byte_threshold: u64,
    /// Maximum objects to sweep per incremental step
    pub incremental_batch: usize,
    /// Enable compaction after sweep?
    pub enable_compaction: bool,
    /// Minimum generations before an object can be collected
    pub min_generations: u32,
}

impl Default for GcConfig {
    fn default() -> Self {
        GcConfig {
            object_threshold: 100_000,
            byte_threshold: 1024 * 1024 * 1024, // 1 GiB
            incremental_batch: 1000,
            enable_compaction: true,
            min_generations: 0,
        }
    }
}

/// The Prism Garbage Collector.
pub struct GarbageCollector {
    /// All tracked objects
    pub objects: BTreeMap<Oid, GcObject>,
    /// Root set (snapshot roots, open handles, pinned objects)
    pub root_set: BTreeSet<Oid>,
    /// Gray set (worklist for tracing)
    gray_set: Vec<Oid>,
    /// Objects to free (after sweep)
    free_list: Vec<(Oid, BlockAddr, u64)>, // (oid, block, size)
    /// Current phase
    pub phase: GcPhase,
    /// Configuration
    pub config: GcConfig,
    /// Statistics
    pub stats: GcStats,
}

impl GarbageCollector {
    pub fn new() -> Self {
        GarbageCollector {
            objects: BTreeMap::new(),
            root_set: BTreeSet::new(),
            gray_set: Vec::new(),
            free_list: Vec::new(),
            phase: GcPhase::Idle,
            config: GcConfig::default(),
            stats: GcStats::default(),
        }
    }

    /// Register a new object with the GC.
    pub fn track(&mut self, oid: Oid, block_addr: BlockAddr, size: u64, references: Vec<Oid>) {
        self.objects.insert(oid, GcObject {
            oid,
            color: GcColor::White,
            block_addr,
            size,
            references,
            external_refs: 0,
            last_access: 0,
            generation: 0,
        });
        self.stats.objects_tracked = self.objects.len() as u64;
    }

    /// Mark an object as a root (will not be collected).
    pub fn add_root(&mut self, oid: Oid) {
        self.root_set.insert(oid);
    }

    /// Remove an object from the root set.
    pub fn remove_root(&mut self, oid: Oid) {
        self.root_set.remove(&oid);
    }

    /// Increment external reference count (open handle, snapshot pin).
    pub fn add_ref(&mut self, oid: Oid) {
        if let Some(obj) = self.objects.get_mut(&oid) {
            obj.external_refs = obj.external_refs.saturating_add(1);
        }
    }

    /// Decrement external reference count.
    pub fn release_ref(&mut self, oid: Oid) {
        if let Some(obj) = self.objects.get_mut(&oid) {
            obj.external_refs = obj.external_refs.saturating_sub(1);
        }
    }

    /// Check if a GC cycle should be triggered.
    pub fn should_collect(&self) -> bool {
        let total_bytes: u64 = self.objects.values().map(|o| o.size).sum();
        self.stats.objects_tracked >= self.config.object_threshold
            || total_bytes >= self.config.byte_threshold
    }

    /// Run a full GC cycle: mark → trace → sweep.
    pub fn collect(&mut self) -> (u64, u64) {
        self.phase = GcPhase::RootDiscovery;

        // Phase 1: Reset all objects to White
        for obj in self.objects.values_mut() {
            obj.color = GcColor::White;
        }

        // Phase 2: Mark root set as Gray
        self.phase = GcPhase::Marking;
        self.gray_set.clear();
        for &root_oid in &self.root_set {
            if let Some(obj) = self.objects.get_mut(&root_oid) {
                obj.color = GcColor::Gray;
                self.gray_set.push(root_oid);
            }
        }

        // Also mark objects with external references
        let externally_held: Vec<Oid> = self.objects.values()
            .filter(|o| o.external_refs > 0 && o.color == GcColor::White)
            .map(|o| o.oid)
            .collect();
        for oid in externally_held {
            if let Some(obj) = self.objects.get_mut(&oid) {
                obj.color = GcColor::Gray;
                self.gray_set.push(oid);
            }
        }

        // Phase 3: Trace — process gray objects until none remain
        while let Some(oid) = self.gray_set.pop() {
            // Get children to mark
            let children: Vec<Oid> = self.objects.get(&oid)
                .map(|o| o.references.clone())
                .unwrap_or_default();

            // Mark self as Black (fully traced)
            if let Some(obj) = self.objects.get_mut(&oid) {
                obj.color = GcColor::Black;
            }

            // Mark children as Gray if still White
            for child_oid in children {
                if let Some(child) = self.objects.get_mut(&child_oid) {
                    if child.color == GcColor::White {
                        child.color = GcColor::Gray;
                        self.gray_set.push(child_oid);
                    }
                }
            }
        }

        // Phase 4: Sweep — collect White objects
        self.phase = GcPhase::Sweeping;
        self.free_list.clear();

        let to_collect: Vec<Oid> = self.objects.values()
            .filter(|o| o.color == GcColor::White && o.generation >= self.config.min_generations)
            .map(|o| o.oid)
            .collect();

        let mut collected_count = 0u64;
        let mut collected_bytes = 0u64;

        for oid in to_collect {
            if let Some(obj) = self.objects.remove(&oid) {
                self.free_list.push((obj.oid, obj.block_addr, obj.size));
                collected_bytes += obj.size;
                collected_count += 1;
            }
        }

        // Increment generation for surviving objects
        for obj in self.objects.values_mut() {
            obj.generation = obj.generation.saturating_add(1);
        }

        // Update stats
        self.stats.cycles += 1;
        self.stats.objects_collected += collected_count;
        self.stats.bytes_reclaimed += collected_bytes;
        self.stats.last_collected = collected_count;
        self.stats.last_reachable = self.objects.len() as u64;
        self.stats.objects_tracked = self.objects.len() as u64;

        self.phase = GcPhase::Idle;

        (collected_count, collected_bytes)
    }

    /// Get the list of blocks to free (after a sweep).
    pub fn drain_free_list(&mut self) -> Vec<(Oid, BlockAddr, u64)> {
        core::mem::take(&mut self.free_list)
    }

    /// Get current statistics.
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }
}
