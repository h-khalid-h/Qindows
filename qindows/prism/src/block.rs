//! # Block Device Abstraction
//!
//! Provides a unified interface for all block storage devices
//! (NVMe, SATA, VirtIO-blk). The Prism storage engine talks
//! to this layer instead of directly to device drivers.

use alloc::string::String;
use alloc::vec::Vec;

/// Block size (Qindows uses 4KB blocks to align with page size).
pub const BLOCK_SIZE: usize = 4096;

/// Block address type.
pub type BlockAddr = u64;

/// A block device trait — implemented by all storage drivers.
pub trait BlockDevice {
    /// Read a single block.
    fn read_block(&mut self, addr: BlockAddr, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError>;

    /// Write a single block.
    fn write_block(&mut self, addr: BlockAddr, buf: &[u8; BLOCK_SIZE]) -> Result<(), BlockError>;

    /// Flush pending writes.
    fn flush(&mut self) -> Result<(), BlockError>;

    /// Get total number of blocks.
    fn total_blocks(&self) -> u64;

    /// Get device name.
    fn name(&self) -> &str;
}

/// Block I/O errors.
#[derive(Debug, Clone)]
pub enum BlockError {
    /// Device not ready
    NotReady,
    /// Invalid block address (out of range)
    InvalidAddress(BlockAddr),
    /// Hardware I/O error
    IoError(String),
    /// Device disconnected
    Disconnected,
    /// Write protection
    ReadOnly,
}

/// Read multiple contiguous blocks.
pub fn read_blocks(
    dev: &mut dyn BlockDevice,
    start: BlockAddr,
    count: u64,
    buffer: &mut Vec<u8>,
) -> Result<(), BlockError> {
    buffer.resize((count as usize) * BLOCK_SIZE, 0);
    let mut block = [0u8; BLOCK_SIZE];

    for i in 0..count {
        dev.read_block(start + i, &mut block)?;
        let offset = (i as usize) * BLOCK_SIZE;
        buffer[offset..offset + BLOCK_SIZE].copy_from_slice(&block);
    }

    Ok(())
}

/// Write multiple contiguous blocks.
pub fn write_blocks(
    dev: &mut dyn BlockDevice,
    start: BlockAddr,
    data: &[u8],
) -> Result<(), BlockError> {
    let num_blocks = (data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;

    for i in 0..num_blocks {
        let offset = i * BLOCK_SIZE;
        let end = (offset + BLOCK_SIZE).min(data.len());
        let mut block = [0u8; BLOCK_SIZE]; // Zero-fill for partial blocks
        block[..end - offset].copy_from_slice(&data[offset..end]);
        dev.write_block(start + i as u64, &block)?;
    }

    dev.flush()
}

/// RAM disk — an in-memory block device for testing.
pub struct RamDisk {
    blocks: Vec<[u8; BLOCK_SIZE]>,
    name: String,
}

impl RamDisk {
    /// Create a new RAM disk with the given number of blocks.
    pub fn new(num_blocks: usize) -> Self {
        RamDisk {
            blocks: alloc::vec![[0u8; BLOCK_SIZE]; num_blocks],
            name: String::from("ramdisk0"),
        }
    }
}

impl BlockDevice for RamDisk {
    fn read_block(&mut self, addr: BlockAddr, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), BlockError> {
        let idx = addr as usize;
        if idx >= self.blocks.len() {
            return Err(BlockError::InvalidAddress(addr));
        }
        buf.copy_from_slice(&self.blocks[idx]);
        Ok(())
    }

    fn write_block(&mut self, addr: BlockAddr, buf: &[u8; BLOCK_SIZE]) -> Result<(), BlockError> {
        let idx = addr as usize;
        if idx >= self.blocks.len() {
            return Err(BlockError::InvalidAddress(addr));
        }
        self.blocks[idx].copy_from_slice(buf);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        Ok(()) // RAM is always "persisted"
    }

    fn total_blocks(&self) -> u64 {
        self.blocks.len() as u64
    }

    fn name(&self) -> &str {
        &self.name
    }
}
