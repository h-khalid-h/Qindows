//! # IOMMU — DMA Remapping per Silo
//!
//! Hardware-level memory isolation for device DMA (Section 9.2).
//! Each Silo gets its own I/O page table, preventing rogue
//! devices or drivers from accessing another Silo's memory.
//!
//! Features:
//! - Per-Silo I/O address spaces
//! - DMA-capable device assignment to Silos
//! - Interrupt remapping (MSI-X isolation)
//! - Fault logging (DMA violations reported to Sentinel)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// DMA mapping type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapType {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

/// A DMA mapping entry.
#[derive(Debug, Clone)]
pub struct DmaMapping {
    pub iova: u64,
    pub phys_addr: u64,
    pub size: u64,
    pub map_type: MapType,
    pub device_id: u32,
    pub silo_id: u64,
}

/// A DMA fault record.
#[derive(Debug, Clone)]
pub struct DmaFault {
    pub id: u64,
    pub device_id: u32,
    pub iova: u64,
    pub fault_type: u8,
    pub silo_id: u64,
    pub timestamp: u64,
}

/// IOMMU statistics.
#[derive(Debug, Clone, Default)]
pub struct IommuStats {
    pub mappings_created: u64,
    pub mappings_removed: u64,
    pub faults: u64,
    pub devices_assigned: u64,
}

/// The IOMMU Manager.
pub struct Iommu {
    pub page_tables: BTreeMap<u64, Vec<DmaMapping>>,
    pub device_owners: BTreeMap<u32, u64>,
    pub fault_log: Vec<DmaFault>,
    next_fault_id: u64,
    pub max_faults: usize,
    pub stats: IommuStats,
}

impl Iommu {
    pub fn new() -> Self {
        Iommu {
            page_tables: BTreeMap::new(),
            device_owners: BTreeMap::new(),
            fault_log: Vec::new(),
            next_fault_id: 1,
            max_faults: 1000,
            stats: IommuStats::default(),
        }
    }

    /// Assign a device to a Silo.
    pub fn assign_device(&mut self, device_id: u32, silo_id: u64) -> Result<(), &'static str> {
        if self.device_owners.contains_key(&device_id) {
            return Err("Device already assigned");
        }
        self.device_owners.insert(device_id, silo_id);
        self.stats.devices_assigned += 1;
        Ok(())
    }

    /// Create a DMA mapping.
    pub fn map(&mut self, silo_id: u64, device_id: u32, iova: u64, phys_addr: u64, size: u64, map_type: MapType) -> Result<(), &'static str> {
        match self.device_owners.get(&device_id) {
            Some(&owner) if owner == silo_id => {}
            Some(_) => return Err("Device not assigned to this Silo"),
            None => return Err("Device not assigned"),
        }

        let table = self.page_tables.entry(silo_id).or_insert_with(Vec::new);
        let overlap = table.iter().any(|m| {
            m.device_id == device_id &&
            m.iova < iova + size && m.iova + m.size > iova
        });
        if overlap {
            return Err("Overlapping IOVA range");
        }

        table.push(DmaMapping { iova, phys_addr, size, map_type, device_id, silo_id });
        self.stats.mappings_created += 1;
        Ok(())
    }

    /// Remove a DMA mapping.
    pub fn unmap(&mut self, silo_id: u64, device_id: u32, iova: u64) {
        if let Some(table) = self.page_tables.get_mut(&silo_id) {
            let before = table.len();
            table.retain(|m| !(m.device_id == device_id && m.iova == iova));
            if table.len() < before {
                self.stats.mappings_removed += 1;
            }
        }
    }

    /// Translate an IOVA to physical address.
    pub fn translate(&self, silo_id: u64, device_id: u32, iova: u64, is_write: bool) -> Option<u64> {
        let table = self.page_tables.get(&silo_id)?;
        table.iter().find(|m| {
            m.device_id == device_id &&
            iova >= m.iova && iova < m.iova + m.size &&
            match (is_write, m.map_type) {
                (true, MapType::ReadOnly) => false,
                (false, MapType::WriteOnly) => false,
                _ => true,
            }
        }).map(|m| m.phys_addr + (iova - m.iova))
    }

    /// Record a DMA fault.
    pub fn record_fault(&mut self, device_id: u32, iova: u64, is_write: bool, now: u64) {
        let silo_id = self.device_owners.get(&device_id).copied().unwrap_or(0);
        let id = self.next_fault_id;
        self.next_fault_id += 1;

        self.fault_log.push(DmaFault {
            id, device_id, iova,
            fault_type: if is_write { 1 } else { 0 },
            silo_id, timestamp: now,
        });

        if self.fault_log.len() > self.max_faults {
            self.fault_log.remove(0);
        }
        self.stats.faults += 1;
    }
}
