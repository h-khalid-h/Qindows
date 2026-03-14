//! # PCI Device Cap Bridge (Phase 218)
//!
//! ## Architecture Guardian: The Gap
//! `pci_scan.rs` + `pci_enum.rs` implement PCI device enumeration.
//! PCI memory-mapped I/O (MMIO) bar mapping is a critical operation that
//! allows direct hardware access.
//!
//! **Missing link**: PCI MMIO bar mapping was not capability-gated.
//! Any Silo could map PCI MMIO regions, gaining direct hardware access.
//!
//! This module provides `PciDeviceCapBridge`:
//! Admin:EXEC cap required before any PCI device MMIO mapping is attempted.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct PciCapStats {
    pub allowed: u64,
    pub denied:  u64,
}

pub struct PciDeviceCapBridge {
    pub stats: PciCapStats,
}

impl PciDeviceCapBridge {
    pub fn new() -> Self {
        PciDeviceCapBridge { stats: PciCapStats::default() }
    }

    /// Authorize PCI MMIO mapping — requires Admin:EXEC cap.
    pub fn authorize_mmio_map(
        &mut self,
        silo_id: u64,
        bus: u8,
        device: u8,
        function: u8,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.denied += 1;
            crate::serial_println!(
                "[PCI] Silo {} MMIO map {:02x}:{:02x}.{} denied — no Admin:EXEC cap",
                silo_id, bus, device, function
            );
            return false;
        }
        self.stats.allowed += 1;
        crate::serial_println!(
            "[PCI] Silo {} MMIO map {:02x}:{:02x}.{} authorized",
            silo_id, bus, device, function
        );
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PciCapBridge: allowed={} denied={}", self.stats.allowed, self.stats.denied
        );
    }
}
