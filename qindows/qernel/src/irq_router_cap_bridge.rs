//! # IRQ Router Cap Bridge (Phase 197)
//!
//! ## Architecture Guardian: The Gap
//! `irq_router.rs` implements `SiloInterruptRouter`:
//! - `allocate_vectors(silo_id, device_id, count:u8, affinity_cpu, cap:&CapToken, tick)` → Result<Vec<u8>>
//! - `route_interrupt(vector: u8)` → Option<u64>
//! - Already has internal CapToken validation via `validate_capability()`
//!
//! **Missing link**: `allocate_vectors()` validates caps internally but
//! was never given a quota per Silo. The global vector table could be
//! exhausted by a single Silo (Law 4 DoS).
//!
//! This module provides `IrqRouterCapBridge`:
//! Adds a 32 vector/Silo quota layer on top of the existing cap validation.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::irq_router::SiloInterruptRouter;
use crate::capability::CapToken;

const MAX_VECTORS_PER_SILO: u8 = 32;

#[derive(Debug, Default, Clone)]
pub struct IrqRouterCapStats {
    pub allocs_ok:      u64,
    pub allocs_denied:  u64,
    pub routes:         u64,
}

pub struct IrqRouterCapBridge {
    pub router:    SiloInterruptRouter,
    silo_counts:   BTreeMap<u64, u8>,
    pub stats:     IrqRouterCapStats,
}

impl IrqRouterCapBridge {
    pub fn new() -> Self {
        IrqRouterCapBridge { router: SiloInterruptRouter::new(), silo_counts: BTreeMap::new(), stats: IrqRouterCapStats::default() }
    }

    /// Allocate IRQ vectors — quota-gated at 32/Silo, cap validated internally.
    pub fn allocate_vectors(
        &mut self,
        silo_id: u64,
        device_id: u32,
        count: u8,
        affinity_cpu: u8,
        cap: &CapToken,
        tick: u64,
    ) -> Option<Vec<u8>> {
        let used = *self.silo_counts.get(&silo_id).unwrap_or(&0);
        if used.saturating_add(count) > MAX_VECTORS_PER_SILO {
            self.stats.allocs_denied += 1;
            crate::serial_println!("[IRQ] Silo {} quota exceeded: {}/{}", silo_id, used, MAX_VECTORS_PER_SILO);
            return None;
        }
        match self.router.allocate_vectors(silo_id, device_id, count, affinity_cpu, cap, tick) {
            Ok(vectors) => {
                *self.silo_counts.entry(silo_id).or_default() += count;
                self.stats.allocs_ok += 1;
                Some(vectors)
            }
            Err(_) => {
                self.stats.allocs_denied += 1;
                None
            }
        }
    }

    pub fn route_interrupt(&mut self, vector: u8) -> Option<u64> {
        self.stats.routes += 1;
        self.router.route_interrupt(vector)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IrqRouterBridge: ok={} denied={} routes={}",
            self.stats.allocs_ok, self.stats.allocs_denied, self.stats.routes
        );
    }
}
