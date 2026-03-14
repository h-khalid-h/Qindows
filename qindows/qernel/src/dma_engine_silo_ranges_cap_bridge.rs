#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use crate::dma_engine::DmaEngine;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

/// Bridge for Phase 293: DMA Engine Silo Ranges Cap Bridge
/// Requres `Admin:EXEC` to map specific physical ranges to a silo for DMA.
pub struct DmaEngineSiloRangesCapBridge<'a> {
    target: &'a mut DmaEngine,
}

impl<'a> DmaEngineSiloRangesCapBridge<'a> {
    pub fn new(target: &'a mut DmaEngine) -> Self {
        Self { target }
    }

    pub fn set_silo_ranges(
        &mut self,
        silo_id: u64,
        target_silo_id: u64,
        ranges: Vec<(u64, u64)>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.target.stats.transfers_failed += 1;
            crate::serial_println!(
                "[DMA ENGINE] Silo {} set_silo_ranges for Silo {} denied — Admin:EXEC required", 
                silo_id, target_silo_id
            );
            return false;
        }

        self.target.set_silo_ranges(target_silo_id, ranges);
        true
    }
}
