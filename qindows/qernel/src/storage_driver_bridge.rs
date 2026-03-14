//! # AHCI + NVMe Disk Scheduler Bridge (Phase 147)
//!
//! ## Architecture Guardian: The Gap
//! `drivers/ahci.rs` implements `AhciController::read_sectors()`/`write_sectors()`
//! `drivers/nvme.rs` implements `NvmeController::read_blocks()`/`submit()`
//!
//! **Missing link**: Both drivers issued I/O directly — never through
//! `DiskScheduler`. This bypassed Silo priority, quota enforcement, and
//! the I/O fairness system entirely.
//!
//! This module provides `StorageDriverBridge`:
//! 1. `read_sata()` — submits via DiskSchedSiloBridge (priority-gated)
//! 2. `write_sata()` — same
//! 3. `read_nvme()` — same for NVMe
//! 4. `write_nvme()` — same

extern crate alloc;

use crate::disk_sched_silo_bridge::DiskSchedSiloBridge;
use crate::disk_sched::IoDir;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct StorageBridgeStats {
    pub sata_reads:  u64,
    pub sata_writes: u64,
    pub nvme_reads:  u64,
    pub nvme_writes: u64,
}

const AHCI_DEVICE_ID: u32 = 0x1000; // SATA device 0
const NVME_DEVICE_ID: u32 = 0x2000; // NVMe device 0

// ── Storage Driver Bridge ─────────────────────────────────────────────────────

/// Routes AHCI and NVMe I/O through DiskSchedSiloBridge for priority + quota.
pub struct StorageDriverBridge {
    pub sched: DiskSchedSiloBridge,
    pub stats: StorageBridgeStats,
}

impl StorageDriverBridge {
    pub fn new() -> Self {
        StorageDriverBridge {
            sched: DiskSchedSiloBridge::new(),
            stats: StorageBridgeStats::default(),
        }
    }

    /// Submit a SATA read through the scheduler.
    pub fn read_sata(
        &mut self,
        silo_id: u64,
        lba: u64,
        sector_count: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.sata_reads += 1;
        self.sched.submit_gated(silo_id, AHCI_DEVICE_ID, IoDir::Read, lba, sector_count, forge, tick)
    }

    /// Submit a SATA write through the scheduler.
    pub fn write_sata(
        &mut self,
        silo_id: u64,
        lba: u64,
        sector_count: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.sata_writes += 1;
        self.sched.submit_gated(silo_id, AHCI_DEVICE_ID, IoDir::Write, lba, sector_count, forge, tick)
    }

    /// Submit an NVMe read through the scheduler.
    pub fn read_nvme(
        &mut self,
        silo_id: u64,
        lba: u64,
        block_count: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.nvme_reads += 1;
        self.sched.submit_gated(silo_id, NVME_DEVICE_ID, IoDir::Read, lba, block_count, forge, tick)
    }

    /// Submit an NVMe write through the scheduler.
    pub fn write_nvme(
        &mut self,
        silo_id: u64,
        lba: u64,
        block_count: u32,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> u64 {
        self.stats.nvme_writes += 1;
        self.sched.submit_gated(silo_id, NVME_DEVICE_ID, IoDir::Write, lba, block_count, forge, tick)
    }

    /// Register Silo at spawn; weight=100 for normal, 200 for service Silos.
    pub fn on_silo_spawn(&mut self, silo_id: u64, weight: u32) {
        self.sched.on_silo_spawn(silo_id, weight);
    }

    /// Clean up Silo's I/O state on vaporize.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.sched.on_silo_vaporize(silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  StorageBridge: sata_r={} sata_w={} nvme_r={} nvme_w={}",
            self.stats.sata_reads, self.stats.sata_writes,
            self.stats.nvme_reads, self.stats.nvme_writes
        );
    }
}
