//! # Chimera Handle Quota Bridge (Phase 162)
//!
//! ## Architecture Guardian: The Gap
//! `chimera.rs` implements `HandleTable`:
//! - `alloc(entry)` → handle: u64
//! - `get(handle)` → Option<&HandleEntry>
//! - `close(handle)` → bool
//!
//! **Missing link**: `chimera_create_file()` allocated Win32-compatible
//! handles with no limit — a Silo running legacy Win32 code could exhaust
//! the handle table, leaking kernel memory and starving other Silos.
//! Handles were never charged against `CGroupManager` or `QQuota`.
//!
//! This module provides `ChimeraHandleQuotaBridge`:
//! 1. `create_with_quota()` — adds to quota before alloc, rejects if over limit
//! 2. `close_with_quota()` — releases quota on close
//! 3. `on_silo_vaporize()` — bulk-releases all Silo handles

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::chimera::{HandleTable, HandleEntry};

const MAX_HANDLES_PER_SILO: u32 = 16_384;

#[derive(Debug, Default, Clone)]
pub struct HandleQuotaStats {
    pub allocs_allowed: u64,
    pub allocs_denied:  u64,
    pub closes:         u64,
    pub leak_reclaimed: u64,
}

pub struct ChimeraHandleQuotaBridge {
    pub table:     HandleTable,
    /// Per-Silo handle count for quota enforcement
    silo_counts:   BTreeMap<u64, u32>,
    /// Per-Silo handle list for vaporize cleanup
    silo_handles:  BTreeMap<u64, Vec<u64>>,
    pub stats:     HandleQuotaStats,
}

impl ChimeraHandleQuotaBridge {
    pub fn new() -> Self {
        ChimeraHandleQuotaBridge {
            table: HandleTable::new(),
            silo_counts: BTreeMap::new(),
            silo_handles: BTreeMap::new(),
            stats: HandleQuotaStats::default(),
        }
    }

    /// Allocate a Win32 handle, enforcing per-Silo quota.
    pub fn create_with_quota(&mut self, silo_id: u64, entry: HandleEntry) -> Option<u64> {
        let count = self.silo_counts.get(&silo_id).copied().unwrap_or(0);
        if count >= MAX_HANDLES_PER_SILO {
            self.stats.allocs_denied += 1;
            crate::serial_println!(
                "[CHIMERA QUOTA] Silo {} hit handle limit ({}/{}) — denied",
                silo_id, count, MAX_HANDLES_PER_SILO
            );
            return None;
        }

        let handle = self.table.alloc(entry);
        *self.silo_counts.entry(silo_id).or_insert(0) += 1;
        self.silo_handles.entry(silo_id).or_default().push(handle);

        self.stats.allocs_allowed += 1;
        Some(handle)
    }

    /// Close a Win32 handle, releasing quota.
    pub fn close_with_quota(&mut self, silo_id: u64, handle: u64) -> bool {
        let closed = self.table.close(handle);
        if closed {
            self.stats.closes += 1;
            if let Some(count) = self.silo_counts.get_mut(&silo_id) {
                *count = count.saturating_sub(1);
            }
        }
        closed
    }

    /// Bulk-close all handles for a vaporized Silo to prevent leaks.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        if let Some(handles) = self.silo_handles.remove(&silo_id) {
            let count = handles.len() as u64;
            for h in handles {
                self.table.close(h);
            }
            self.silo_counts.remove(&silo_id);
            self.stats.leak_reclaimed += count;
            crate::serial_println!(
                "[CHIMERA QUOTA] Silo {} vaporized: {} handles reclaimed", silo_id, count
            );
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ChimeraHandleBridge: allowed={} denied={} closes={} reclaimed={}",
            self.stats.allocs_allowed, self.stats.allocs_denied,
            self.stats.closes, self.stats.leak_reclaimed
        );
    }
}
