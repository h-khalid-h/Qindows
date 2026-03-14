//! # KProbe Admin Cap Bridge (Phase 238)
//!
//! ## Architecture Guardian: The Gap
//! `kprobe.rs` implements `KProbeManager`:
//! - `add(name, probe_type: ProbeType, target_addr: u64, now)` → Result<u64>
//! - `hit(addr, latency_ns, now)` — record probe hit
//! - `ProbeType` — Ftrace, Uprobe, Kretprobe, XdpHook
//!
//! **Missing link**: KProbe insertion at arbitrary kernel addresses
//! was not capability-gated. Any Silo could instrument the kernel,
//! exfiltrating secrets via probe data.
//!
//! This module provides `KProbeAdminCapBridge`:
//! Admin:EXEC cap required before any KProbe is inserted.

extern crate alloc;

use crate::kprobe::{KProbeManager, ProbeType};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct KProbeCapStats {
    pub probes_allowed: u64,
    pub probes_denied:  u64,
}

pub struct KProbeAdminCapBridge {
    pub manager: KProbeManager,
    pub stats:   KProbeCapStats,
}

impl KProbeAdminCapBridge {
    pub fn new() -> Self {
        KProbeAdminCapBridge { manager: KProbeManager::new(), stats: KProbeCapStats::default() }
    }

    pub fn add_probe(
        &mut self,
        silo_id: u64,
        name: &str,
        probe_type: ProbeType,
        target_addr: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.probes_denied += 1;
            crate::serial_println!("[KPROBE] Silo {} insert denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        match self.manager.add(name, probe_type, target_addr, tick) {
            Ok(_) => { self.stats.probes_allowed += 1; true }
            Err(e) => { crate::serial_println!("[KPROBE] add failed: {}", e); false }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KProbeCapBridge: allowed={} denied={}",
            self.stats.probes_allowed, self.stats.probes_denied
        );
    }
}
