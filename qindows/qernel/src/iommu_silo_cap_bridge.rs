//! # IOMMU Silo Cap Bridge (Phase 196)
//!
//! ## Architecture Guardian: The Gap
//! `iommu.rs` implements `Iommu`:
//! - `assign_device(device_id, silo_id)` → Result<(), &str>
//! - `map(silo_id, device_id, iova, phys_addr, size, map_type)` → Result<(), &str>
//! - `MapType` enum
//!
//! **Missing link**: `assign_device()` and `map()` were callable without Admin:EXEC cap.
//! A Silo could map DMA addresses into another Silo's physical memory — a critical
//! memory isolation violation (Law 6: Silo Sandbox).
//!
//! This module provides `IommuSiloCapBridge`:
//! Admin:EXEC cap required for all IOMMU device assignment and mapping operations.

extern crate alloc;

use crate::iommu::{Iommu, MapType};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct IommuCapStats {
    pub assigns_allowed: u64,
    pub assigns_denied:  u64,
    pub maps_allowed:    u64,
    pub maps_denied:     u64,
}

pub struct IommuSiloCapBridge {
    pub iommu: Iommu,
    pub stats: IommuCapStats,
}

impl IommuSiloCapBridge {
    pub fn new() -> Self {
        IommuSiloCapBridge { iommu: Iommu::new(), stats: IommuCapStats::default() }
    }

    /// Assign a device to a Silo — requires Admin:EXEC cap.
    pub fn assign_device(
        &mut self,
        silo_id: u64,
        device_id: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.assigns_denied += 1;
            crate::serial_println!("[IOMMU] Silo {} assign denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.assigns_allowed += 1;
        self.iommu.assign_device(device_id, silo_id).is_ok()
    }

    /// Map IOVA → physical address — requires Admin:EXEC cap.
    pub fn map(
        &mut self,
        silo_id: u64,
        device_id: u32,
        iova: u64,
        phys_addr: u64,
        size: u64,
        map_type: MapType,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.maps_denied += 1;
            return false;
        }
        self.stats.maps_allowed += 1;
        self.iommu.map(silo_id, device_id, iova, phys_addr, size, map_type).is_ok()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IommuCapBridge: assign={}/{} map={}/{}",
            self.stats.assigns_allowed, self.stats.assigns_denied,
            self.stats.maps_allowed, self.stats.maps_denied
        );
    }
}
