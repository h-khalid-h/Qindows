//! # DMA Engine
//!
//! Direct Memory Access controller for high-throughput data transfers
//! without CPU involvement. Used by NVMe, AHCI, HDA audio, and
//! VirtIO drivers for zero-copy I/O.

use alloc::vec::Vec;

/// DMA transfer direction.
#[derive(Debug, Clone, Copy)]
pub enum DmaDirection {
    /// Device → Memory
    FromDevice,
    /// Memory → Device
    ToDevice,
    /// Bidirectional
    Bidirectional,
}

/// A physical memory region for DMA (scatter-gather entry).
#[derive(Debug, Clone, Copy)]
pub struct DmaRegion {
    /// Physical address (must be aligned)
    pub phys_addr: u64,
    /// Length in bytes
    pub length: u64,
}

/// A DMA buffer — contiguous or scatter-gather.
#[derive(Debug, Clone)]
pub struct DmaBuffer {
    /// Scatter-gather list
    pub regions: Vec<DmaRegion>,
    /// Total size (sum of all regions)
    pub total_size: u64,
    /// Direction
    pub direction: DmaDirection,
    /// Is this buffer currently mapped?
    pub mapped: bool,
}

impl DmaBuffer {
    /// Allocate a single contiguous DMA buffer.
    pub fn contiguous(phys_addr: u64, size: u64, direction: DmaDirection) -> Self {
        DmaBuffer {
            regions: alloc::vec![DmaRegion { phys_addr, length: size }],
            total_size: size,
            direction,
            mapped: false,
        }
    }

    /// Create a scatter-gather DMA buffer.
    pub fn scatter_gather(regions: Vec<DmaRegion>, direction: DmaDirection) -> Self {
        let total = regions.iter().map(|r| r.length).sum();
        DmaBuffer {
            regions,
            total_size: total,
            direction,
            mapped: false,
        }
    }
}

/// DMA channel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    Idle,
    Active,
    Paused,
    Error,
}

/// A DMA channel (one transfer path).
#[derive(Debug, Clone)]
pub struct DmaChannel {
    /// Channel index
    pub index: u8,
    /// State
    pub state: ChannelState,
    /// Current buffer
    pub buffer: Option<DmaBuffer>,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
    /// Transfer complete callback ID
    pub callback_id: Option<u64>,
    /// Priority (0=highest)
    pub priority: u8,
    /// Transfer statistics
    pub total_transfers: u64,
    pub total_bytes: u64,
}

impl DmaChannel {
    pub fn new(index: u8) -> Self {
        DmaChannel {
            index,
            state: ChannelState::Idle,
            buffer: None,
            bytes_transferred: 0,
            callback_id: None,
            priority: 4, // Default mid-priority
            total_transfers: 0,
            total_bytes: 0,
        }
    }

    /// Start a DMA transfer.
    pub fn start(&mut self, buffer: DmaBuffer) {
        self.buffer = Some(buffer);
        self.state = ChannelState::Active;
        self.bytes_transferred = 0;
    }

    /// Mark transfer as complete.
    pub fn complete(&mut self) {
        if let Some(ref buf) = self.buffer {
            self.total_bytes += buf.total_size;
        }
        self.total_transfers += 1;
        self.state = ChannelState::Idle;
        self.buffer = None;
    }

    /// Pause the transfer.
    pub fn pause(&mut self) {
        if self.state == ChannelState::Active {
            self.state = ChannelState::Paused;
        }
    }

    /// Resume a paused transfer.
    pub fn resume(&mut self) {
        if self.state == ChannelState::Paused {
            self.state = ChannelState::Active;
        }
    }

    /// Is this channel busy?
    pub fn is_busy(&self) -> bool {
        self.state == ChannelState::Active || self.state == ChannelState::Paused
    }
}

/// The DMA Controller.
pub struct DmaController {
    /// Available channels
    pub channels: Vec<DmaChannel>,
    /// IOMMU base address (for address translation)
    pub iommu_base: Option<u64>,
    /// Total bytes transferred across all channels
    pub total_bytes: u64,
    /// Is the controller initialized?
    pub initialized: bool,
}

impl DmaController {
    /// Initialize the DMA controller.
    pub fn init(num_channels: u8) -> Self {
        let mut channels = Vec::new();
        for i in 0..num_channels {
            channels.push(DmaChannel::new(i));
        }

        crate::serial_println!("[OK] DMA controller: {} channels", num_channels);

        DmaController {
            channels,
            iommu_base: None,
            total_bytes: 0,
            initialized: true,
        }
    }

    /// Allocate a free DMA channel.
    pub fn alloc_channel(&mut self) -> Option<u8> {
        self.channels.iter()
            .find(|c| c.state == ChannelState::Idle)
            .map(|c| c.index)
    }

    /// Submit a DMA transfer.
    pub fn submit(
        &mut self,
        channel: u8,
        buffer: DmaBuffer,
        callback_id: Option<u64>,
    ) -> Result<(), &'static str> {
        let ch = self.channels.get_mut(channel as usize)
            .ok_or("Invalid channel")?;

        if ch.is_busy() {
            return Err("Channel busy");
        }

        ch.callback_id = callback_id;
        ch.start(buffer);

        Ok(())
    }

    /// Handle DMA completion interrupt.
    pub fn handle_completion(&mut self, channel: u8) -> Option<u64> {
        let ch = self.channels.get_mut(channel as usize)?;

        if let Some(ref buf) = ch.buffer {
            self.total_bytes += buf.total_size;
        }

        let callback = ch.callback_id;
        ch.complete();
        callback
    }

    /// Translate a virtual address to physical (via IOMMU).
    pub fn translate(&self, virt_addr: u64) -> u64 {
        if let Some(iommu) = self.iommu_base {
            // In production: walk the IOMMU page tables
            let _ = iommu;
            virt_addr // Identity mapping for now
        } else {
            virt_addr
        }
    }

    /// Get throughput statistics.
    pub fn throughput_stats(&self) -> (u64, u64) {
        let total_transfers: u64 = self.channels.iter().map(|c| c.total_transfers).sum();
        (total_transfers, self.total_bytes)
    }
}
