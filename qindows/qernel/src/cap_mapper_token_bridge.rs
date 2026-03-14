//! # Capability Mapper Token Bridge (Phase 155)
//!
//! ## Architecture Guardian: The Gap
//! `memory/cap_mapper.rs` implements:
//! - `map_permissions_for_cap(token, current_tick)` → MapPermissions
//! - `kernel_code_permissions()` → MapPermissions (read+exec, no write)
//! - `kernel_rodata_permissions()` → MapPermissions (read-only)
//! - `mmio_permissions()` → MapPermissions (read+write, no exec, no cache)
//!
//! **Missing link**: `map_permissions_for_cap()` derived page table
//! permissions from a CapToken — but it was never called during Silo launch.
//! Silos were spawned with default RWX pages regardless of their CapToken.
//!
//! This module provides `CapMapperTokenBridge`:
//! 1. `permissions_for_silo()` — calls map_permissions_for_cap() at spawn
//! 2. `kernel_mapping()` — returns correct kernel code permissions
//! 3. `mmio_mapping()` — returns MMIO permission set

use crate::memory::cap_mapper::{
    map_permissions_for_cap, kernel_code_permissions, kernel_rodata_permissions, mmio_permissions,
    MapPermissions,
};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC, CAP_READ};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct CapMapperStats {
    pub silo_mappings:   u64,
    pub kernel_mappings: u64,
    pub mmio_mappings:   u64,
}

// ── Cap Mapper Token Bridge ───────────────────────────────────────────────────

/// Calls cap_mapper at Silo spawn to derive correct page table permissions.
pub struct CapMapperTokenBridge {
    pub stats: CapMapperStats,
}

impl CapMapperTokenBridge {
    pub fn new() -> Self {
        CapMapperTokenBridge { stats: CapMapperStats::default() }
    }

    /// Get page-table permissions for a Silo page based on its CapToken.
    /// Called by silo_launch.rs during CR3/page-table setup.
    pub fn permissions_for_silo(
        &mut self,
        silo_id: u64,
        forge: &CapTokenForge,
        tick: u64,
    ) -> MapPermissions {
        self.stats.silo_mappings += 1;

        // Retrieve the Silo's active CapToken and derive page permissions
        if let Some(token) = forge.get_token(silo_id) {
            let perms = map_permissions_for_cap(token, tick);
            crate::serial_println!(
                "[CAP MAPPER] Silo {} page perms: exec={} write={} read={}",
                silo_id, perms.execute, perms.write, perms.read
            );
            perms
        } else {
            // No token → read-only user pages (minimal privilege)
            crate::serial_println!("[CAP MAPPER] Silo {} has no token — read-only pages", silo_id);
            kernel_rodata_permissions() // safest default
        }
    }

    /// Standard kernel code mapping (read+exec, no write).
    pub fn kernel_code(&mut self) -> MapPermissions {
        self.stats.kernel_mappings += 1;
        kernel_code_permissions()
    }

    /// Read-only kernel data section mapping.
    pub fn kernel_rodata(&mut self) -> MapPermissions {
        self.stats.kernel_mappings += 1;
        kernel_rodata_permissions()
    }

    /// MMIO uncached read+write mapping.
    pub fn mmio(&mut self) -> MapPermissions {
        self.stats.mmio_mappings += 1;
        mmio_permissions()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CapMapper: silo_maps={} kernel_maps={} mmio_maps={}",
            self.stats.silo_mappings, self.stats.kernel_mappings, self.stats.mmio_mappings
        );
    }
}
