//! # QFS Ghost Retention Bridge (Phase 175)
//!
//! ## Architecture Guardian: The Gap
//! `qfs_ghost.rs` implements `PrismObjectStore`:
//! - `new()` — creates store
//! - `create(author_silo, current_tick)` → u64 (object_id)
//! - `ghost_write(object_id, data, author_silo, tick)` — writes a version
//! - `read_head(object_id)` → Option<&PrismVersion>
//! - `read_version(object_id, version)` → Option<&PrismVersion>
//!
//! **Missing link**: Prism object writes and reads were never gated by CapToken.
//! Prism:WRITE and Prism:READ were defined in CapType but never enforced on
//! the underlying object store writes.
//!
//! This module provides `QfsGhostRetentionBridge`:
//! Prism:WRITE gate on `create` + `ghost_write`, Prism:READ gate on `read_head`.

extern crate alloc;
use alloc::vec::Vec;

use crate::qfs_ghost::{PrismObjectStore, PrismVersion};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ, CAP_WRITE};

#[derive(Debug, Default, Clone)]
pub struct GhostRetentionStats {
    pub writes_allowed: u64,
    pub writes_denied:  u64,
    pub reads_allowed:  u64,
    pub reads_denied:   u64,
}

pub struct QfsGhostRetentionBridge {
    pub store: PrismObjectStore,
    pub stats: GhostRetentionStats,
}

impl QfsGhostRetentionBridge {
    pub fn new() -> Self {
        QfsGhostRetentionBridge { store: PrismObjectStore::new(), stats: GhostRetentionStats::default() }
    }

    /// Create a new Prism object — requires Prism:WRITE cap.
    pub fn create_with_cap_check(
        &mut self,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(silo_id, CapType::Prism, CAP_WRITE, 0, tick) {
            self.stats.writes_denied += 1;
            crate::serial_println!("[GHOST] Silo {} Prism create denied — no Prism:WRITE cap", silo_id);
            return None;
        }
        self.stats.writes_allowed += 1;
        Some(self.store.create(silo_id, tick))
    }

    /// Write a new version to a Prism object — requires Prism:WRITE cap.
    pub fn ghost_write_with_cap_check(
        &mut self,
        silo_id: u64,
        object_id: u64,
        data_phys: u64,   // physical NVMe LBA address of the new data block
        data_size: u32,   // size in bytes
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_WRITE, 0, tick) {
            self.stats.writes_denied += 1;
            return false;
        }
        self.stats.writes_allowed += 1;
        self.store.ghost_write(object_id, data_phys, data_size, silo_id, tick).is_ok()
    }

    /// Read the latest Prism object version — requires Prism:READ cap.
    pub fn read_head_with_cap_check(
        &mut self,
        silo_id: u64,
        object_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<&PrismVersion> {
        if !forge.check(silo_id, CapType::Prism, CAP_READ, 0, tick) {
            self.stats.reads_denied += 1;
            return None;
        }
        self.stats.reads_allowed += 1;
        self.store.read_head(object_id)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GhostRetentionBridge: writes={}/{} reads={}/{}",
            self.stats.writes_allowed, self.stats.writes_denied,
            self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
