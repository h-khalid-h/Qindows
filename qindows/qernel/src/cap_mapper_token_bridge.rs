//! # Capability Mapper Token Bridge (Phase 155)
//!
//! ## Architecture Guardian: The Gap
//! `memory/cap_mapper.rs` provides:
//! - `map_permissions_for_cap(token: &CapToken, tick)` → MapPermissions
//! - Uses `crate::capability::CapToken` (not cap_tokens::CapTokenForge)
//! - `MapPermissions` is from `crate::memory::vmm`
//!
//! **Missing link**: Called at Silo launch to derive page table permissions
//! from a CapToken — but the call chain was never wired in `silo_launch.rs`.
//! Silos were spawned with default RWX pages regardless of their CapToken.
//!
//! This module provides `CapMapperTokenBridge`:
//! 1. `permissions_for_cap()` — direct wrapper around map_permissions_for_cap
//! 2. `kernel_code()/kernel_rodata()/mmio()` — kernel mapping helpers
//! Both are now callable from silo_launch.rs

use crate::memory::cap_mapper::{
    map_permissions_for_cap, kernel_code_permissions,
    kernel_rodata_permissions, mmio_permissions,
};
use crate::memory::vmm::MapPermissions;
use crate::capability::CapToken;

#[derive(Debug, Default, Clone)]
pub struct CapMapperStats {
    pub silo_mappings:   u64,
    pub kernel_mappings: u64,
    pub mmio_mappings:   u64,
}

pub struct CapMapperTokenBridge {
    pub stats: CapMapperStats,
}

impl CapMapperTokenBridge {
    pub fn new() -> Self {
        CapMapperTokenBridge { stats: CapMapperStats::default() }
    }

    /// Derive page-table permissions from a Silo's CapToken.
    /// Called by silo_launch.rs during page table setup.
    pub fn permissions_for_cap(&mut self, token: &CapToken, tick: u64) -> MapPermissions {
        self.stats.silo_mappings += 1;
        map_permissions_for_cap(token, tick)
    }

    /// Kernel code: read+exec, no write.
    pub fn kernel_code(&mut self) -> MapPermissions {
        self.stats.kernel_mappings += 1;
        kernel_code_permissions()
    }

    /// Read-only kernel data.
    pub fn kernel_rodata(&mut self) -> MapPermissions {
        self.stats.kernel_mappings += 1;
        kernel_rodata_permissions()
    }

    /// MMIO: uncached read+write.
    pub fn mmio(&mut self) -> MapPermissions {
        self.stats.mmio_mappings += 1;
        mmio_permissions()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CapMapper: silo={} kernel={} mmio={}",
            self.stats.silo_mappings, self.stats.kernel_mappings, self.stats.mmio_mappings
        );
    }
}
