//! # Prism Live Index Eviction Bridge (Phase 254)
//!
//! ## Architecture Guardian: The Gap
//! `prism_live_index.rs` implements `LiveObjectIndex`:
//! - `register(oid, silo_id, size, ...)` — register a new object
//! - `update_version(oid, new_size, tick)` — update object version
//! - `evict_silo(silo_id)` — remove all objects from a Silo
//! - `lookup(oid)` → Option<&ObjectMeta>
//!
//! **Missing link**: `register()` had no per-Silo object count cap.
//! A Silo could fill the entire LiveObjectIndex with ephemeral objects,
//! causing memory pressure and evicting other Silos' objects.
//!
//! This module provides `PrismLiveIndexEvictionBridge`:
//! Max 1024 live objects per Silo.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::prism_live_index::LiveObjectIndex;
use crate::qaudit_kernel::QAuditKernel;

const MAX_OBJECTS_PER_SILO: u64 = 1024;

#[derive(Debug, Default, Clone)]
pub struct PrismLiveIndexStats {
    pub registers_allowed: u64,
    pub registers_denied:  u64,
}

pub struct PrismLiveIndexEvictionBridge {
    silo_object_counts: BTreeMap<u64, u64>,
    pub stats:          PrismLiveIndexStats,
}

impl PrismLiveIndexEvictionBridge {
    pub fn new() -> Self {
        PrismLiveIndexEvictionBridge { silo_object_counts: BTreeMap::new(), stats: PrismLiveIndexStats::default() }
    }

    pub fn allow_register(&mut self, silo_id: u64, audit: &mut QAuditKernel, tick: u64) -> bool {
        let count = self.silo_object_counts.entry(silo_id).or_default();
        if *count >= MAX_OBJECTS_PER_SILO {
            self.stats.registers_denied += 1;
            audit.log_law_violation(4u8, silo_id, tick); // Law 4: resource fairness
            crate::serial_println!(
                "[PRISM IDX] Silo {} object quota exceeded ({}/{})", silo_id, count, MAX_OBJECTS_PER_SILO
            );
            return false;
        }
        *count += 1;
        self.stats.registers_allowed += 1;
        true
    }

    pub fn on_evict_silo(&mut self, index: &mut LiveObjectIndex, silo_id: u64) {
        index.evict_silo(silo_id);
        self.silo_object_counts.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PrismLiveIndexBridge: allowed={} denied={}",
            self.stats.registers_allowed, self.stats.registers_denied
        );
    }
}
