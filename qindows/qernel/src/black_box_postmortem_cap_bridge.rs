//! # Black Box PostMortem Cap Bridge (Phase 225)
//!
//! ## Architecture Guardian: The Gap
//! `black_box.rs` implements the kernel flight recorder:
//! - `TraceEvent { kind: TraceEventKind, silo_id, ... }` — kernel events
//! - `PostMortemObject::compute_behaviour_hash()` → [u8; 32]
//! - `SyscallLogEntry` — per-syscall audit record
//!
//! **Missing link**: `PostMortemObject` and `TraceEvent` reads from other
//! Silos were unprotected. Any Silo could read another Silo's syscall trace.
//!
//! This module provides `BlackBoxPostMortemCapBridge`:
//! Admin:EXEC cap required to read any PostMortem or TraceEvent data.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct BlackBoxCapStats {
    pub reads_allowed: u64,
    pub reads_denied:  u64,
}

pub struct BlackBoxPostMortemCapBridge {
    pub stats: BlackBoxCapStats,
}

impl BlackBoxPostMortemCapBridge {
    pub fn new() -> Self {
        BlackBoxPostMortemCapBridge { stats: BlackBoxCapStats::default() }
    }

    /// Authorize reading a PostMortem or trace log — requires Admin:EXEC cap.
    pub fn authorize_read(
        &mut self,
        reader_silo: u64,
        target_silo: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Own Silo can always read its own trace
        if reader_silo == target_silo {
            self.stats.reads_allowed += 1;
            return true;
        }
        if !forge.check(reader_silo, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.reads_denied += 1;
            crate::serial_println!(
                "[BLACK BOX] Silo {} denied reading Silo {} trace — no Admin:EXEC cap",
                reader_silo, target_silo
            );
            return false;
        }
        self.stats.reads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  BlackBoxBridge: allowed={} denied={}", self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
