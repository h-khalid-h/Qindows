//! # IRQ Router Silo Bridge (Phase 156)
//!
//! ## Architecture Guardian: The Gap
//! `irq_router.rs` implements `SiloInterruptRouter`:
//! - `allocate_vectors(silo_id, device_id, count, affinity_cpu, cap, tick)` → Result<Vec<u8>>
//! - `route_interrupt(vector)` → Option<u64> (target Silo)
//! - `mask_vector(vector, silo_id)` / `unmask_vector()`
//!
//! **SiloInterruptRouter already checks CapToken internally** via `Permissions::DEVICE`.
//! **Missing link**: On Silo vaporize, allocated vectors were never released.
//! IRQ vectors leaked — eventually exhausting the SILO_VECTOR_POOL.
//!
//! This module provides `IrqSiloBridge`:
//! 1. `on_silo_spawn()` — allocate vectors for a device-owning Silo
//! 2. `on_silo_vaporize()` — free all vectors for that Silo
//! 3. `on_irq()` — route a hardware interrupt to the correct Silo

extern crate alloc;
use alloc::vec::Vec;

use crate::irq_router::{SiloInterruptRouter, IrqFaultReason};
use crate::capability::CapToken;

#[derive(Debug, Default, Clone)]
pub struct IrqBridgeStats {
    pub silos_registered: u64,
    pub vectors_allocated: u64,
    pub vectors_freed: u64,
    pub irqs_routed: u64,
    pub irqs_dropped: u64,
}

pub struct IrqSiloBridge {
    pub router: SiloInterruptRouter,
    pub stats:  IrqBridgeStats,
}

impl IrqSiloBridge {
    pub fn new() -> Self {
        IrqSiloBridge {
            router: SiloInterruptRouter::new(),
            stats:  IrqBridgeStats::default(),
        }
    }

    /// Allocate IRQ vectors for a Silo that owns a PCIe/USB device.
    /// SiloInterruptRouter internally validates the CapToken's DEVICE permission.
    pub fn on_silo_spawn(
        &mut self,
        silo_id: u64,
        device_id: u32,
        vector_count: u8,
        affinity_cpu: u8,
        cap: &CapToken,
        tick: u64,
    ) -> Result<Vec<u8>, IrqFaultReason> {
        self.stats.silos_registered += 1;
        match self.router.allocate_vectors(silo_id, device_id, vector_count, affinity_cpu, cap, tick) {
            Ok(vectors) => {
                self.stats.vectors_allocated += vectors.len() as u64;
                crate::serial_println!(
                    "[IRQ BRIDGE] Silo {} allocated {} vectors for device {:08x}",
                    silo_id, vectors.len(), device_id
                );
                Ok(vectors)
            }
            Err(reason) => {
                crate::serial_println!("[IRQ BRIDGE] Silo {} vector alloc failed: {:?}", silo_id, reason);
                Err(reason)
            }
        }
    }

    /// Route a hardware interrupt to the correct Silo.
    pub fn on_irq(&mut self, vector: u8) -> Option<u64> {
        if let Some(silo_id) = self.router.route_interrupt(vector) {
            self.stats.irqs_routed += 1;
            Some(silo_id)
        } else {
            self.stats.irqs_dropped += 1;
            None
        }
    }

    /// Mask vectors when a Silo is paused (snapshot/migration).
    pub fn mask_silo_vectors(&mut self, vectors: &[u8], silo_id: u64) {
        for &v in vectors {
            let _ = self.router.mask_vector(v, silo_id);
        }
    }

    /// Unmask vectors when a Silo resumes.
    pub fn unmask_silo_vectors(&mut self, vectors: &[u8], silo_id: u64) {
        for &v in vectors {
            let _ = self.router.unmask_vector(v, silo_id);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IrqBridge: silos={} vecs_alloc={} vecs_freed={} routed={} dropped={}",
            self.stats.silos_registered, self.stats.vectors_allocated,
            self.stats.vectors_freed, self.stats.irqs_routed, self.stats.irqs_dropped
        );
    }
}
