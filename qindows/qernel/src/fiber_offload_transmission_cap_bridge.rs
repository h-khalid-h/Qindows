//! # Fiber Offload Transmission Cap Bridge (Phase 257)
//!
//! ## Architecture Guardian: The Gap
//! `fiber_offload.rs` implements edge-kernel Fiber serialization:
//! - `FiberSnapshot::transmission_bytes()` → u64 — serialized size
//! - `FiberSnapshot::prism_savings_bytes()` → u64 — dedup savings
//! - `OffloadPhase` — Serialize, Transmit, Activate, Complete
//!
//! **Missing link**: Fiber transmission had no per-Silo bandwidth cap.
//! A Silo with a huge JIT code cache could serialize a 1 GB Fiber
//! snapshot and saturate the Nexus uplink.
//!
//! This module provides `FiberOffloadTransmissionCapBridge`:
//! Max 64 MiB per Fiber snapshot transmission.

extern crate alloc;

use crate::fiber_offload::FiberSnapshot;
use crate::qaudit_kernel::QAuditKernel;

const MAX_FIBER_TRANSMISSION_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

#[derive(Debug, Default, Clone)]
pub struct FiberOffloadCapStats {
    pub allowed: u64,
    pub denied:  u64,
}

pub struct FiberOffloadTransmissionCapBridge {
    pub stats: FiberOffloadCapStats,
}

impl FiberOffloadTransmissionCapBridge {
    pub fn new() -> Self {
        FiberOffloadTransmissionCapBridge { stats: FiberOffloadCapStats::default() }
    }

    pub fn authorize_transmit(
        &mut self,
        snapshot: &FiberSnapshot,
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        let size = snapshot.transmission_bytes();
        if size > MAX_FIBER_TRANSMISSION_BYTES {
            self.stats.denied += 1;
            audit.log_law_violation(4u8, silo_id, tick);
            crate::serial_println!(
                "[FIBER] Silo {} snapshot {} bytes exceeds {} MiB cap", silo_id, size, MAX_FIBER_TRANSMISSION_BYTES / (1024*1024)
            );
            return false;
        }
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FiberOffloadCapBridge: allowed={} denied={}", self.stats.allowed, self.stats.denied
        );
    }
}
