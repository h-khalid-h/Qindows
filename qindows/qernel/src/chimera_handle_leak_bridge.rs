//! # Chimera Handle Leak Bridge (Phase 264)
//!
//! ## Architecture Guardian: The Gap
//! `chimera.rs` implements Win32 handle translation:
//! - `HandleTable { entries: Vec<HandleEntry> }`
//! - `HandleTable::alloc(entry)` → u64 — allocate Win32 handle
//! - `HandleTable::get(handle)` → Option<&HandleEntry>
//! - `HandleKind` — File, Event, Thread, Process, ...
//!
//! **Missing link**: Win32 handle allocation was unbounded. A Chimera
//! (Win32 compat) workload could allocate millions of handles, exhausting
//! the kernel handle table and starving native Q-Silos.
//!
//! This module provides `ChimeraHandleLeakBridge`:
//! Max 4096 handles per Silo — tracks at allocation granularity.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_HANDLES_PER_SILO: u64 = 4096;

#[derive(Debug, Default, Clone)]
pub struct ChimeraHandleStats {
    pub allocs_allowed: u64,
    pub allocs_denied:  u64,
}

pub struct ChimeraHandleLeakBridge {
    silo_handle_counts: BTreeMap<u64, u64>,
    pub stats:          ChimeraHandleStats,
}

impl ChimeraHandleLeakBridge {
    pub fn new() -> Self {
        ChimeraHandleLeakBridge { silo_handle_counts: BTreeMap::new(), stats: ChimeraHandleStats::default() }
    }

    pub fn allow_alloc(&mut self, silo_id: u64) -> bool {
        let count = self.silo_handle_counts.entry(silo_id).or_default();
        if *count >= MAX_HANDLES_PER_SILO {
            self.stats.allocs_denied += 1;
            crate::serial_println!(
                "[CHIMERA] Silo {} handle quota full ({}/{})", silo_id, count, MAX_HANDLES_PER_SILO
            );
            return false;
        }
        *count += 1;
        self.stats.allocs_allowed += 1;
        true
    }

    pub fn on_handle_free(&mut self, silo_id: u64) {
        let count = self.silo_handle_counts.entry(silo_id).or_default();
        *count = count.saturating_sub(1);
    }

    pub fn on_vaporize(&mut self, silo_id: u64) {
        self.silo_handle_counts.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ChimeraHandleLeak: allowed={} denied={}",
            self.stats.allocs_allowed, self.stats.allocs_denied
        );
    }
}
