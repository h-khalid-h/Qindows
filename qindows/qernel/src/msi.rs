//! # MSI/MSI-X Interrupt Controller
//!
//! Manages Message-Signaled Interrupts for PCIe devices,
//! replacing legacy IRQ lines (Section 9.16).
//!
//! Features:
//! - MSI (single vector) and MSI-X (multi-vector) support
//! - Per-device vector allocation
//! - Target CPU affinity for interrupt steering
//! - IRQ-to-vector mapping
//! - Interrupt coalescing

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// MSI type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiType {
    Msi,
    MsiX,
}

/// An MSI/MSI-X vector allocation.
#[derive(Debug, Clone)]
pub struct MsiVector {
    pub vector: u16,
    pub device_id: u32,
    pub target_cpu: u32,
    pub msi_type: MsiType,
    pub masked: bool,
    pub pending: bool,
    pub fires: u64,
}

/// MSI controller statistics.
#[derive(Debug, Clone, Default)]
pub struct MsiStats {
    pub vectors_allocated: u64,
    pub interrupts_delivered: u64,
    pub interrupts_coalesced: u64,
    pub vectors_masked: u64,
}

/// The MSI Controller.
pub struct MsiController {
    /// Vector → allocation
    pub vectors: BTreeMap<u16, MsiVector>,
    /// Device → vector list
    pub device_vectors: BTreeMap<u32, Vec<u16>>,
    next_vector: u16,
    pub base_vector: u16,
    pub max_vectors: u16,
    pub stats: MsiStats,
}

impl MsiController {
    pub fn new(base_vector: u16, max_vectors: u16) -> Self {
        MsiController {
            vectors: BTreeMap::new(),
            device_vectors: BTreeMap::new(),
            next_vector: base_vector,
            base_vector,
            max_vectors,
            stats: MsiStats::default(),
        }
    }

    /// Allocate MSI vectors for a device.
    pub fn allocate(&mut self, device_id: u32, count: u16, msi_type: MsiType, target_cpu: u32) -> Vec<u16> {
        let mut allocated = Vec::new();

        for _ in 0..count {
            if self.next_vector >= self.base_vector + self.max_vectors {
                break; // No more vectors
            }

            let vec = self.next_vector;
            self.next_vector += 1;

            self.vectors.insert(vec, MsiVector {
                vector: vec, device_id, target_cpu, msi_type,
                masked: false, pending: false, fires: 0,
            });

            allocated.push(vec);
            self.stats.vectors_allocated += 1;
        }

        self.device_vectors.entry(device_id)
            .or_insert_with(Vec::new)
            .extend_from_slice(&allocated);

        allocated
    }

    /// Signal an interrupt on a vector.
    pub fn signal(&mut self, vector: u16) -> Option<u32> {
        let v = self.vectors.get_mut(&vector)?;
        if v.masked {
            v.pending = true;
            return None;
        }
        v.fires += 1;
        self.stats.interrupts_delivered += 1;
        Some(v.target_cpu)
    }

    /// Mask a vector.
    pub fn mask(&mut self, vector: u16) {
        if let Some(v) = self.vectors.get_mut(&vector) {
            v.masked = true;
            self.stats.vectors_masked += 1;
        }
    }

    /// Unmask a vector. Returns target CPU if pending interrupt.
    pub fn unmask(&mut self, vector: u16) -> Option<u32> {
        let v = self.vectors.get_mut(&vector)?;
        v.masked = false;
        if v.pending {
            v.pending = false;
            v.fires += 1;
            self.stats.interrupts_delivered += 1;
            Some(v.target_cpu)
        } else { None }
    }

    /// Free all vectors for a device.
    pub fn free_device(&mut self, device_id: u32) {
        if let Some(vecs) = self.device_vectors.remove(&device_id) {
            for v in vecs {
                self.vectors.remove(&v);
            }
        }
    }
}
