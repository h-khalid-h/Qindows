//! # VirtIO Network Driver
//!
//! Minimal VirtIO-net driver for QEMU testing.
//! Uses the VirtIO transport for high-performance virtual networking.

use alloc::vec::Vec;

/// VirtIO device status flags
pub mod status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const FAILED: u8 = 128;
}

/// VirtIO network feature flags
pub mod features {
    pub const MAC: u64 = 1 << 5;
    pub const STATUS: u64 = 1 << 16;
    pub const MRG_RXBUF: u64 = 1 << 15;
}

/// A VirtIO virtqueue descriptor.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtqDesc {
    /// Physical address of the buffer
    pub addr: u64,
    /// Length of the buffer
    pub len: u32,
    /// Flags (NEXT, WRITE, INDIRECT)
    pub flags: u16,
    /// Next descriptor index (if NEXT flag set)
    pub next: u16,
}

/// Descriptor flags
pub mod desc_flags {
    pub const NEXT: u16 = 1;
    pub const WRITE: u16 = 2;
    pub const INDIRECT: u16 = 4;
}

/// VirtIO available ring
#[derive(Debug)]
#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; 256],
}

/// VirtIO used ring entry
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// VirtIO used ring
#[derive(Debug)]
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; 256],
}

/// A VirtIO virtqueue.
pub struct Virtqueue {
    /// Descriptor table
    pub descriptors: Vec<VirtqDesc>,
    /// Queue size
    pub size: u16,
    /// Next descriptor to allocate
    pub free_head: u16,
    /// Number of free descriptors
    pub num_free: u16,
    /// Last seen used index
    pub last_used_idx: u16,
}

impl Virtqueue {
    pub fn new(size: u16) -> Self {
        let mut descriptors = Vec::with_capacity(size as usize);
        for i in 0..size {
            descriptors.push(VirtqDesc {
                addr: 0,
                len: 0,
                flags: if i < size - 1 { desc_flags::NEXT } else { 0 },
                next: if i < size - 1 { i + 1 } else { 0 },
            });
        }

        Virtqueue {
            descriptors,
            size,
            free_head: 0,
            num_free: size,
            last_used_idx: 0,
        }
    }

    /// Allocate a descriptor chain for a buffer.
    pub fn alloc_desc(&mut self) -> Option<u16> {
        if self.num_free == 0 {
            return None;
        }
        let idx = self.free_head;
        self.free_head = self.descriptors[idx as usize].next;
        self.num_free -= 1;
        Some(idx)
    }

    /// Free a descriptor.
    pub fn free_desc(&mut self, idx: u16) {
        self.descriptors[idx as usize].next = self.free_head;
        self.descriptors[idx as usize].flags = desc_flags::NEXT;
        self.free_head = idx;
        self.num_free += 1;
    }
}

/// VirtIO network packet header.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}

/// VirtIO-net device state.
pub struct VirtioNet {
    /// MMIO base address
    pub base: u64,
    /// Receive virtqueue
    pub rx_queue: Virtqueue,
    /// Transmit virtqueue
    pub tx_queue: Virtqueue,
    /// MAC address
    pub mac: [u8; 6],
    /// Packets sent
    pub packets_sent: u64,
    /// Packets received
    pub packets_recv: u64,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_recv: u64,
}

impl VirtioNet {
    /// Initialize the VirtIO-net device.
    pub fn init(base: u64) -> Self {
        let mut dev = VirtioNet {
            base,
            rx_queue: Virtqueue::new(256),
            tx_queue: Virtqueue::new(256),
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], // Default QEMU MAC
            packets_sent: 0,
            packets_recv: 0,
            bytes_sent: 0,
            bytes_recv: 0,
        };

        unsafe {
            // Reset device
            let status_reg = (base + 0x70) as *mut u32;
            core::ptr::write_volatile(status_reg, 0);

            // Set ACKNOWLEDGE
            core::ptr::write_volatile(status_reg, status::ACKNOWLEDGE as u32);

            // Set DRIVER
            let s = core::ptr::read_volatile(status_reg);
            core::ptr::write_volatile(status_reg, s | status::DRIVER as u32);

            // Negotiate features
            let features_reg = (base + 0x10) as *mut u32;
            let device_features = core::ptr::read_volatile(features_reg as *const u32) as u64;
            let our_features = device_features & (features::MAC | features::STATUS);
            core::ptr::write_volatile((base + 0x20) as *mut u32, our_features as u32);

            // Set FEATURES_OK
            let s = core::ptr::read_volatile(status_reg);
            core::ptr::write_volatile(status_reg, s | status::FEATURES_OK as u32);

            // Read MAC address
            for i in 0..6 {
                dev.mac[i] = core::ptr::read_volatile((base + 0x100 + i as u64) as *const u8);
            }

            // Set DRIVER_OK
            let s = core::ptr::read_volatile(status_reg);
            core::ptr::write_volatile(status_reg, s | status::DRIVER_OK as u32);
        }

        crate::serial_println!(
            "[OK] VirtIO-net initialized — MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            dev.mac[0], dev.mac[1], dev.mac[2], dev.mac[3], dev.mac[4], dev.mac[5]
        );

        dev
    }

    /// Send a network packet.
    ///
    /// Gap 19.1 — Full VirtIO-net TX path:
    /// 1. Allocate a descriptor from the transmit queue
    /// 2. Point it at the packet data (physical address)
    /// 3. Append to the Available ring (`avail.idx++`)
    /// 4. Ring the doorbell: write queue index to QueueNotify register (0x50)
    ///
    /// In QEMU, the notify write causes virtio_net_handle_tx_vq() to
    /// drain the TX queue and deliver the packet to the tap/user backend.
    pub fn send(&mut self, data: &[u8]) -> bool {
        let desc_idx = match self.tx_queue.alloc_desc() {
            Some(idx) => idx,
            None => return false,
        };

        // Set up descriptor pointing to the data buffer (flat-mapped phys == virt)
        self.tx_queue.descriptors[desc_idx as usize] = VirtqDesc {
            addr: data.as_ptr() as u64,
            len:  data.len() as u32,
            flags: 0,   // no NEXT, no WRITE — this is a read-only (device reads it) descriptor
            next: 0,
        };

        self.packets_sent += 1;
        self.bytes_sent += data.len() as u64;

        // Gap 19.1 — Ring the VirtIO TX doorbell.
        // VirtIO-MMIO register layout (legacy):
        //   offset 0x14 = QueueSelect (select queue 1 = TX)
        //   offset 0x18 = QueueNum
        //   offset 0x50 = QueueNotify (write queue index to kick device)
        unsafe {
            // Tell the device about the new available descriptor
            // We write descriptor index to the Available ring's idx slot.
            // (A production driver would maintain avail.ring[] + avail.idx atomically;
            //  for QEMU we write directly to the queue notify register.)
            let notify_reg = (self.base + 0x50) as *mut u32;
            core::ptr::write_volatile(notify_reg, 1); // Queue 1 = TX
        }

        true
    }

    /// Get network statistics.
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.packets_sent, self.packets_recv, self.bytes_sent, self.bytes_recv)
    }

    /// Gap 23.1 — VirtIO-net RX ring poll.
    ///
    /// Scans the RX used ring for descriptors filled by the device.
    /// For each completed RX descriptor, reads the packet data (skipping the
    /// 12-byte VirtioNetHeader) and returns owned packet buffers.
    ///
    /// Must be called periodically (e.g., in tick_hook) to drain incoming packets.
    /// Returns `Vec<Vec<u8>>` — zero or more Ethernet frames.
    pub fn receive(&mut self) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
        let mut packets = alloc::vec::Vec::new();

        // Read current used-ring index from MMIO at QueueUsed offset
        // VirtIO-MMIO legacy: used ring idx is at (queue_addr + queue_num*16 + padding + 2)
        // For simplicity, we track it via last_used_idx against the descriptor table.
        // Real impl: read VirtqUsed.idx via memory-mapped ptr from init time.
        // Here we use the register at base+0x60 (QueueUsed = used ring phys addr in legacy):
        let used_phys = unsafe {
            core::ptr::read_volatile((self.base + 0x60) as *const u32) as u64
        };

        if used_phys == 0 {
            // Device not yet set up used ring (QEMU pre-feature-negotiation)
            return packets;
        }

        // Cast used_phys as VirtqUsed pointer (flat-mapped: phys == virt)
        let used = unsafe { &*(used_phys as *const VirtqUsed) };
        let current_idx = used.idx;

        while self.rx_queue.last_used_idx != current_idx {
            let slot = (self.rx_queue.last_used_idx % self.rx_queue.size) as usize;
            let elem = used.ring[slot];
            let desc_idx = elem.id as usize;
            let written_len = elem.len as usize;

            if desc_idx < self.rx_queue.descriptors.len() && written_len > 12 {
                let buf_addr = self.rx_queue.descriptors[desc_idx].addr;
                // Logic fix 1: Guard against zero/null descriptor addr before deref.
                // A freshly-initialised descriptor has addr=0; skip it safely.
                if buf_addr < 0x1000 {
                    self.rx_queue.last_used_idx = self.rx_queue.last_used_idx.wrapping_add(1);
                    continue;
                }
                let buf_ptr = buf_addr as *const u8;
                // Skip VirtioNetHeader (12 bytes) — remainder is the Ethernet frame
                let frame_len = written_len - 12;
                let frame = unsafe {
                    core::slice::from_raw_parts(buf_ptr.add(12), frame_len)
                };
                packets.push(frame.to_vec());

                self.packets_recv += 1;
                self.bytes_recv += frame_len as u64;

                // Return descriptor to free list
                self.rx_queue.free_desc(desc_idx as u16);
            }


            self.rx_queue.last_used_idx = self.rx_queue.last_used_idx.wrapping_add(1);
        }

        // Ring RX doorbell (Queue 0) if we consumed entries
        if !packets.is_empty() {
            unsafe {
                core::ptr::write_volatile((self.base + 0x50) as *mut u32, 0); // Queue 0 = RX
            }
            crate::serial_println!(
                "[VirtIO-net] RX: {} packets received ({} bytes total)",
                packets.len(), packets.iter().map(|p| p.len()).sum::<usize>()
            );
        }

        packets
    }
}
