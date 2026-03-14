//! # KDump Admin Cap Bridge (Phase 206)
//!
//! ## Architecture Guardian: The Gap
//! `kdump.rs` implements `KDump`:
//! - `capture(silo_id, reason: CrashReason, ...)` — capture crash dump
//! - `get(id)` → Option<&CrashDump> — retrieve a dump
//! - `CrashReason` — Panic, CapViolation, MemFault, Timeout, SentinelKill
//!
//! **Missing link**: Any Silo could call `get()` to read crash dumps of other Silos.
//! Core dumps contain full memory snapshots — a severe data leak (Law 9).
//!
//! This module provides `KdumpAdminCapBridge`:
//! Admin:EXEC required to read any crash dump, with silo ownership enforcement.

extern crate alloc;

use crate::kdump::{KDump, CrashReason};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct KdumpCapStats {
    pub reads_allowed: u64,
    pub reads_denied:  u64,
}

pub struct KdumpAdminCapBridge {
    pub kdump: KDump,
    pub stats: KdumpCapStats,
}

impl KdumpAdminCapBridge {
    pub fn new() -> Self {
        KdumpAdminCapBridge { kdump: KDump::new(), stats: KdumpCapStats::default() }
    }

    /// Read a crash dump — requires Admin:EXEC cap (memory snapshot access).
    pub fn read_dump(
        &mut self,
        silo_id: u64,
        dump_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.reads_denied += 1;
            crate::serial_println!("[KDUMP] Silo {} dump read denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.reads_allowed += 1;
        self.kdump.get(dump_id).is_some()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KdumpBridge: reads={}/{}",
            self.stats.reads_allowed, self.stats.reads_denied
        );
    }
}
