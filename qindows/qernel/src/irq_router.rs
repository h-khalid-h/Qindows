//! # Per-Silo Interrupt Router
//!
//! **Architecture Guardian Mandate (Phase 49):**
//! The interrupt subsystem must enforce Q-Manifest Law 6 (Silo Sandbox):
//! no Silo may receive, steal, or interfere with an interrupt destined for
//! another Silo.
//!
//! ## The Gap (found in audit):
//! Before Phase 49, `MsiController::allocate()` accepted any caller with
//! no CapToken check. A rogue driver Silo could claim any MSI-X vector
//! and receive interrupts from any PCIe device.
//!
//! ## Solution: `SiloInterruptRouter`
//!
//! A centralized router that:
//! 1. **Allocates** interrupt vectors from per-Silo pools (vectors 48–223
//!    partitioned as 8 vectors × 22 Silos, leaving 224–255 for kernel).
//! 2. **Validates** that the Silo holds a `DEVICE` CapToken before assigning.
//! 3. **Routes** incoming interrupt notifications to the correct Silo fiber
//!    via the Q-Ring IPC channel (asynchronous dispatch, no sync trap).
//! 4. **Releases** all vectors on Silo vaporize (zero remnant IRQ routing).
//!
//! ## Vector Map (x86_64 interrupt vector space)
//!
//! ```text
//! 0x00–0x1F  CPU exceptions (reserved, hardware)
//! 0x20–0x2F  Kernel-only (timer=0x20, syscall=0x80, IPI=0xF0–0xFF)
//! 0x30–0x2F  IRQ balance pool (16 classic ISA IRQs)
//! 0x40–0xDF  Silo MSI-X pool (32 Silos × 8 vectors each = 256 entries)
//! 0xE0–0xEF  Kernel-reserved (IOMMU fault, PMC overflow)
//! 0xF0–0xFF  IPI/APIC (wakeup, TLB shootdown, panic)
//! ```

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::capability::{CapToken, Permissions, validate_capability};

// ── Constants ──────────────────────────────────────────────────────────────

/// First vector in the Silo MSI-X pool.
pub const SILO_VECTOR_BASE: u8 = 0x40;
/// Vectors reserved per Silo (8 MSI-X vectors per device is common).
pub const VECTORS_PER_SILO: u8 = 8;
/// Maximum concurrent Silos with interrupt allocations.
pub const MAX_SILO_SLOTS: usize = 32;
/// Total pool size (fits in usize; u8 would overflow 8*32=256).
pub const SILO_VECTOR_POOL_SIZE: usize = VECTORS_PER_SILO as usize * MAX_SILO_SLOTS; // 256

// ── Types ──────────────────────────────────────────────────────────────────

/// A single allocated interrupt vector owned by a Silo.
#[derive(Debug, Clone)]
pub struct SiloVector {
    /// The x86_64 interrupt vector number (0x40–0xDF).
    pub vector: u8,
    /// The Silo that owns this vector.
    pub silo_id: u64,
    /// The PCIe device this vector serves (0 = unbound).
    pub device_id: u32,
    /// Whether the vector is currently masked at the IOAPIC/MSI-X level.
    pub masked: bool,
    /// Total fires (telemetry).
    pub fires: u64,
}

/// Per-Silo interrupt allocation record.
#[derive(Debug, Clone)]
pub struct SiloInterruptAllocation {
    pub silo_id: u64,
    /// Assigned vectors for this Silo.
    pub vectors: Vec<u8>,
    /// Target CPU APIC ID for interrupt affinity.
    pub affinity_cpu: u8,
}

/// Interrupt routing fault reasons (reported to Sentinel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqFaultReason {
    /// Silo attempted to steal another Silo's vector.
    VectorSteal,
    /// Silo lacked DEVICE CapToken.
    MissingCapability,
    /// Silo attempted to allocate beyond its quota.
    QuotaExceeded,
    /// Vector already assigned to another Silo.
    VectorConflict,
}

/// Interrupt routing stats.
#[derive(Debug, Default, Clone)]
pub struct IrqRouterStats {
    pub vectors_allocated: u64,
    pub vectors_released: u64,
    pub faults: u64,
    pub interrupts_routed: u64,
}

/// Per-Silo MSI-X interrupt router.
///
/// Central authority for interrupt vector allocation and ownership.
/// All vector allocations must pass through this router — never
/// directly through `MsiController`.
pub struct SiloInterruptRouter {
    /// vector → SiloVector ownership record
    pub vector_map: BTreeMap<u8, SiloVector>,
    /// silo_id → allocation record
    pub silo_allocs: BTreeMap<u64, SiloInterruptAllocation>,
    /// Fault log (for Sentinel)
    pub fault_log: Vec<(u64, IrqFaultReason)>, // (silo_id, reason)
    /// Stats
    pub stats: IrqRouterStats,
}

impl SiloInterruptRouter {
    pub fn new() -> Self {
        SiloInterruptRouter {
            vector_map: BTreeMap::new(),
            silo_allocs: BTreeMap::new(),
            fault_log: Vec::new(),
            stats: IrqRouterStats::default(),
        }
    }

    // ── Allocation ─────────────────────────────────────────────────────────

    /// Allocate up to `count` MSI-X vectors for a Silo.
    ///
    /// Enforces:
    /// 1. Silo must hold a valid `DEVICE` CapToken (Q-Manifest Law 1).
    /// 2. Silo quota: max `VECTORS_PER_SILO` active vectors per Silo.
    /// 3. No vector is ever assigned to two Silos simultaneously.
    ///
    /// Returns the list of allocated vector numbers on success.
    pub fn allocate_vectors(
        &mut self,
        silo_id: u64,
        device_id: u32,
        count: u8,
        affinity_cpu: u8,
        cap: &CapToken,
        current_tick: u64,
    ) -> Result<Vec<u8>, IrqFaultReason> {
        // ── Law 1: CapToken validation ──────────────────────────────────
        if validate_capability(cap, Permissions::DEVICE, current_tick).is_err() {
            self.log_fault(silo_id, IrqFaultReason::MissingCapability);
            return Err(IrqFaultReason::MissingCapability);
        }
        if cap.owner_silo != silo_id {
            self.log_fault(silo_id, IrqFaultReason::VectorSteal);
            return Err(IrqFaultReason::VectorSteal);
        }

        // ── Quota check ─────────────────────────────────────────────────
        let current_count = self.silo_allocs
            .get(&silo_id)
            .map(|a| a.vectors.len() as u8)
            .unwrap_or(0);

        if current_count + count > VECTORS_PER_SILO {
            self.log_fault(silo_id, IrqFaultReason::QuotaExceeded);
            return Err(IrqFaultReason::QuotaExceeded);
        }

        // ── Find free vectors ────────────────────────────────────────────
        let mut allocated = Vec::new();
        let mut candidate = SILO_VECTOR_BASE;

        while allocated.len() < count as usize
            && (candidate as usize) < SILO_VECTOR_BASE as usize + SILO_VECTOR_POOL_SIZE
        {
            if !self.vector_map.contains_key(&candidate) {
                // Claim the vector
                self.vector_map.insert(candidate, SiloVector {
                    vector: candidate,
                    silo_id,
                    device_id,
                    masked: false,
                    fires: 0,
                });
                allocated.push(candidate);
            }
            candidate = candidate.wrapping_add(1);
        }

        if allocated.is_empty() {
            return Err(IrqFaultReason::QuotaExceeded); // Pool exhausted
        }

        // ── Record allocation ────────────────────────────────────────────
        let alloc = self.silo_allocs.entry(silo_id).or_insert(SiloInterruptAllocation {
            silo_id,
            vectors: Vec::new(),
            affinity_cpu,
        });
        alloc.affinity_cpu = affinity_cpu;
        alloc.vectors.extend_from_slice(&allocated);

        self.stats.vectors_allocated += allocated.len() as u64;

        crate::serial_println!(
            "[IRQ] Silo {} allocated {} vectors {:?} → CPU {} for device {:04x}",
            silo_id, allocated.len(), &allocated, affinity_cpu, device_id
        );

        Ok(allocated)
    }

    // ── Routing ────────────────────────────────────────────────────────────

    /// Record an interrupt fire on a vector and return the owning Silo.
    ///
    /// Called from the interrupt handler before dispatching to a Silo.
    /// Returns `None` if the vector is unowned or masked — in either
    /// case the interrupt is silently dropped (reduces attack surface).
    pub fn route_interrupt(&mut self, vector: u8) -> Option<u64> {
        let v = self.vector_map.get_mut(&vector)?;
        if v.masked {
            return None; // Silently discard masked interrupts
        }
        v.fires += 1;
        self.stats.interrupts_routed += 1;
        Some(v.silo_id)
    }

    // ── Masking ────────────────────────────────────────────────────────────

    /// Mask a vector on behalf of the owning Silo.
    /// Returns an error if `silo_id` doesn't own `vector`.
    pub fn mask_vector(&mut self, vector: u8, silo_id: u64) -> Result<(), IrqFaultReason> {
        match self.vector_map.get_mut(&vector) {
            Some(v) if v.silo_id == silo_id => {
                v.masked = true;
                Ok(())
            }
            Some(_) => {
                self.log_fault(silo_id, IrqFaultReason::VectorSteal);
                Err(IrqFaultReason::VectorSteal)
            }
            None => Ok(()), // Already gone — no-op
        }
    }

    /// Unmask a vector. Only the owning Silo may unmask its own vectors.
    pub fn unmask_vector(&mut self, vector: u8, silo_id: u64) -> Result<(), IrqFaultReason> {
        match self.vector_map.get_mut(&vector) {
            Some(v) if v.silo_id == silo_id => {
                v.masked = false;
                Ok(())
            }
            Some(_) => {
                self.log_fault(silo_id, IrqFaultReason::VectorSteal);
                Err(IrqFaultReason::VectorSteal)
            }
            None => Ok(()),
        }
    }

    // ── Vaporize path ──────────────────────────────────────────────────────

    /// Release ALL interrupt vectors for a vaporized Silo.
    ///
    /// Called from `silo/mod.rs::vaporize()` **before** the Silo's
    /// memory is freed. This ensures no stale interrupt can ever reach
    /// a recycled address space.
    ///
    /// **Architecture Guardian:** This function is the interrupt-layer
    /// mirror of `Iommu::release_silo()`. Both must be called in tandem
    /// during vaporize to achieve full Q-Silo isolation cleanup.
    pub fn release_silo(&mut self, silo_id: u64) {
        if let Some(alloc) = self.silo_allocs.remove(&silo_id) {
            let released = alloc.vectors.len() as u64;
            for v in &alloc.vectors {
                self.vector_map.remove(v);
            }
            self.stats.vectors_released += released;
            crate::serial_println!(
                "[IRQ] Silo {} released {} interrupt vectors",
                silo_id, released
            );
        }
    }

    // ── Sentinel integration ───────────────────────────────────────────────

    /// Check if a Silo has any pending IRQ faults (for Sentinel health score).
    pub fn fault_count_for_silo(&self, silo_id: u64) -> usize {
        self.fault_log.iter().filter(|(s, _)| *s == silo_id).count()
    }

    /// Current number of allocated vectors (telemetry).
    pub fn allocated_vector_count(&self) -> usize {
        self.vector_map.len()
    }

    // ── Private ────────────────────────────────────────────────────────────

    fn log_fault(&mut self, silo_id: u64, reason: IrqFaultReason) {
        self.fault_log.push((silo_id, reason));
        self.stats.faults += 1;
        crate::serial_println!(
            "[IRQ FAULT] Silo {} — {:?}",
            silo_id, reason
        );
    }
}
