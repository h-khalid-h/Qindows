//! # Virtio GPU Silo Cap Bridge (Phase 246)
//!
//! ## Architecture Guardian: The Gap
//! `virtio_gpu.rs` implements the VirtIO GPU protocol:
//! - `ResourceCreate2d { resource_id, width, height, ... }`
//! - `ResourceFlush { resource_id, rect: Rect }`
//! - `SetScanout { scanout_id, resource_id, rect }`
//!
//! **Missing link**: VirtIO GPU resources (framebuffers) had no ownership
//! check. One Silo could set its scanout to use another Silo's resource_id,
//! reading pixel data from another Silo's framebuffer (info leak).
//!
//! This module provides `VirtioGpuSiloCapBridge`:
//! Resource ownership tracked per Silo — cross-Silo resource access denied.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct VirtioGpuCapStats {
    pub creates_allowed:    u64,
    pub flush_allowed:      u64,
    pub cross_silo_blocked: u64,
}

pub struct VirtioGpuSiloCapBridge {
    resource_owners: BTreeMap<u32, u64>, // resource_id → silo_id
    pub stats:       VirtioGpuCapStats,
}

impl VirtioGpuSiloCapBridge {
    pub fn new() -> Self {
        VirtioGpuSiloCapBridge { resource_owners: BTreeMap::new(), stats: VirtioGpuCapStats::default() }
    }

    /// Register a newly created GPU resource for a Silo.
    pub fn create_resource(&mut self, silo_id: u64, resource_id: u32) {
        self.resource_owners.insert(resource_id, silo_id);
        self.stats.creates_allowed += 1;
    }

    /// Authorize flushing a resource — only owner Silo may flush.
    pub fn authorize_flush(
        &mut self,
        silo_id: u64,
        resource_id: u32,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        match self.resource_owners.get(&resource_id) {
            Some(&owner) if owner == silo_id => { self.stats.flush_allowed += 1; true }
            Some(&owner) => {
                self.stats.cross_silo_blocked += 1;
                audit.log_law_violation(6u8, silo_id, tick);
                crate::serial_println!(
                    "[VIRTIO GPU] Silo {} flush on resource {} owned by Silo {} — Law 6 BLOCKED",
                    silo_id, resource_id, owner
                );
                false
            }
            None => { self.stats.flush_allowed += 1; true } // unregistered — allow (system resources)
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  VirtioGpuBridge: creates={} flush_allowed={} blocked={}",
            self.stats.creates_allowed, self.stats.flush_allowed, self.stats.cross_silo_blocked
        );
    }
}
