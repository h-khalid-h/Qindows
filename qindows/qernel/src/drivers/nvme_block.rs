//! # NVMe Block Device Adapter
//!
//! Bridges the NVMe controller to Prism's block I/O interface.
//! Converts 4KB block addresses to NVMe LBA commands and manages
//! the controller lifecycle for use by the storage subsystem.

use alloc::string::String;
use spin::Mutex;

use super::nvme::NvmeController;

/// Block size used by Prism (matches page size).
pub const BLOCK_SIZE: usize = 4096;

/// NVMe-backed block device adapter.
///
/// Wraps an `NvmeController` and provides block-level I/O
/// that aligns with Prism's 4KB block abstraction.
pub struct NvmeBlockDevice {
    /// The underlying NVMe controller
    pub controller: NvmeController,
    /// Device name (e.g., "nvme0")
    pub device_name: String,
    /// Blocks per NVMe LBA (BLOCK_SIZE / lba_size)
    blocks_per_lba: u32,
}

impl NvmeBlockDevice {
    /// Create a new block device adapter from an initialized NVMe controller.
    pub fn new(controller: NvmeController, name: &str) -> Self {
        let lba_size = controller.lba_size.max(1);
        let blocks_per_lba = (BLOCK_SIZE as u32) / lba_size;

        NvmeBlockDevice {
            controller,
            device_name: String::from(name),
            blocks_per_lba,
        }
    }

    /// Read a single 4KB block.
    ///
    /// Translates the block address to NVMe LBA and issues a read command.
    pub fn read_block(&mut self, block_addr: u64, buf: &mut [u8; BLOCK_SIZE]) -> bool {
        let lba = block_addr * self.blocks_per_lba as u64;
        let buf_phys = buf.as_ptr() as u64;

        if let Some(_cid) = self.controller.read_blocks(lba, self.blocks_per_lba as u16, buf_phys) {
            // Poll for completion
            if let Some(io_queue) = self.controller.io_queue.as_mut() {
                for _ in 0..100_000u32 {
                    if io_queue.poll_completion().is_some() {
                        return true;
                    }
                    core::hint::spin_loop();
                }
            }
        }
        false
    }

    /// Write a single 4KB block.
    ///
    /// Translates block address to NVMe LBA and issues a write command.
    pub fn write_block(&mut self, block_addr: u64, buf: &[u8; BLOCK_SIZE]) -> bool {
        let lba = block_addr * self.blocks_per_lba as u64;
        let buf_phys = buf.as_ptr() as u64;

        if let Some(_cid) = self.controller.write_blocks(lba, self.blocks_per_lba as u16, buf_phys) {
            if let Some(io_queue) = self.controller.io_queue.as_mut() {
                for _ in 0..100_000u32 {
                    if io_queue.poll_completion().is_some() {
                        return true;
                    }
                    core::hint::spin_loop();
                }
            }
        }
        false
    }

    /// Flush pending writes to persistent storage.
    pub fn flush(&mut self) -> bool {
        if let Some(_cid) = self.controller.flush() {
            if let Some(io_queue) = self.controller.io_queue.as_mut() {
                for _ in 0..100_000u32 {
                    if io_queue.poll_completion().is_some() {
                        return true;
                    }
                    core::hint::spin_loop();
                }
            }
        }
        false
    }

    /// Get total number of 4KB blocks on the device.
    pub fn total_blocks(&self) -> u64 {
        let lba_size = self.controller.lba_size.max(1) as u64;
        (self.controller.namespace_size * lba_size) / BLOCK_SIZE as u64
    }

    /// Get the device name.
    pub fn name(&self) -> &str {
        &self.device_name
    }
}

/// Global NVMe block device — registered during Phase 10 boot.
///
/// Used by the Prism silo to issue storage operations.
pub static NVME_BLOCK_DEVICE: Mutex<Option<NvmeBlockDevice>> = Mutex::new(None);
