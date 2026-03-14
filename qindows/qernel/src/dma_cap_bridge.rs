//! # DMA Engine IOMMU Cap Bridge (Phase 164)
//!
//! ## Architecture Guardian: The Gap
//! `dma_engine.rs` implements `DmaEngine`:
//! - `set_silo_ranges(silo_id, ranges: Vec<(u64, u64)>)` — IOMMU allowed ranges
//! - `queue(silo_id, device_id, direction, sg_list: Vec<SgEntry>, now)` → Result<u64, &str>
//! - `complete(transfer_id, now)` → Result<(), &str>
//!
//! **Missing link**: `DmaEngine::queue()` was never gated by CapToken.
//! A Silo could issue DMA to any physical address — a critical Law 6 violation.
//!
//! This module provides `DmaCapBridge`:
//! 1. `register_silo_ranges()` — sets IOMMU ranges at Silo spawn
//! 2. `queue_with_cap_check()` — Admin:EXEC required + IOMMU validated internally

extern crate alloc;
use alloc::vec::Vec;

use crate::dma_engine::{DmaEngine, DmaDirection, SgEntry};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct DmaBridgeStats {
    pub transfers_allowed: u64,
    pub transfers_denied:  u64,
    pub silos_registered:  u64,
}

pub struct DmaCapBridge {
    pub engine: DmaEngine,
    pub stats:  DmaBridgeStats,
}

impl DmaCapBridge {
    pub fn new() -> Self {
        DmaCapBridge { engine: DmaEngine::new(), stats: DmaBridgeStats::default() }
    }

    /// Register IOMMU-safe memory ranges for a Silo at spawn.
    pub fn register_silo_ranges(&mut self, silo_id: u64, ranges: Vec<(u64, u64)>) {
        self.stats.silos_registered += 1;
        self.engine.set_silo_ranges(silo_id, ranges);
        crate::serial_println!("[DMA CAP] Silo {} IOMMU ranges registered", silo_id);
    }

    /// Queue a scatter-gather DMA transfer — requires Admin:EXEC cap (Law 6).
    pub fn queue_with_cap_check(
        &mut self,
        silo_id: u64,
        device_id: u32,
        direction: DmaDirection,
        sg_list: Vec<SgEntry>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.transfers_denied += 1;
            crate::serial_println!(
                "[DMA CAP] Silo {} DMA denied — no Admin:EXEC cap (Law 6)", silo_id
            );
            return None;
        }
        match self.engine.queue(silo_id, device_id, direction, sg_list, tick) {
            Ok(tid) => {
                self.stats.transfers_allowed += 1;
                Some(tid)
            }
            Err(e) => {
                crate::serial_println!("[DMA CAP] queue() failed: {}", e);
                None
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  DmaBridge: allowed={} denied={} silos={}",
            self.stats.transfers_allowed, self.stats.transfers_denied, self.stats.silos_registered
        );
    }
}
