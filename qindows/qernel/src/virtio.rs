//! # VirtIO Driver Framework
//!
//! Provides the core VirtIO PCI transport and virtqueue semantics
//! for virtualized devices (Section 5.11).
//!
//! Supported devices:
//! - virtio-blk (block storage)
//! - virtio-net (networking)
//! - virtio-gpu (display)
//! - virtio-rng (entropy)

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// VirtIO device status flags.
pub mod status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const DEVICE_NEEDS_RESET: u8 = 64;
    pub const FAILED: u8 = 128;
}

/// Virtqueue descriptor flags.
pub mod vq_flags {
    pub const NEXT: u16 = 1;
    pub const WRITE: u16 = 2;
    pub const INDIRECT: u16 = 4;
}

/// A Virtqueue Descriptor (Split Virtqueue).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// A Virtqueue Available Ring.
#[repr(C, packed)]
pub struct VqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; 256], // Fixed size for now
    pub used_event: u16,
}

/// A Virtqueue Used Element.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VqUsedElem {
    pub id: u32,
    pub len: u32,
}

/// A Virtqueue Used Ring.
#[repr(C, packed)]
pub struct VqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VqUsedElem; 256],
    pub avail_event: u16,
}

/// A Virtqueue representation in the driver.
pub struct VirtQueue {
    pub queue_idx: u16,
    pub queue_size: u16,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used_idx: u16,
    pub free_head: u16,
    pub free_count: u16,
}

impl VirtQueue {
    pub fn new(queue_idx: u16, queue_size: u16, desc: *mut VqDesc, avail: *mut VqAvail, used: *mut VqUsed) -> Self {
        VirtQueue {
            queue_idx,
            queue_size,
            desc,
            avail,
            used,
            last_used_idx: 0,
            free_head: 0,
            free_count: queue_size,
        }
    }

    /// Allocate a descriptor chain.
    pub fn alloc_desc(&mut self, count: u16) -> Option<u16> {
        if self.free_count < count {
            return None;
        }
        let head = self.free_head;
        let mut curr = head;
        for _ in 0..count - 1 {
            unsafe {
                let next = (*self.desc.add(curr as usize)).next;
                curr = next;
            }
        }
        self.free_head = unsafe { (*self.desc.add(curr as usize)).next };
        self.free_count -= count;
        Some(head)
    }

    /// Free a descriptor chain.
    pub fn free_desc(&mut self, head: u16, count: u16) {
        let mut curr = head;
        for _ in 0..count - 1 {
            unsafe {
                curr = (*self.desc.add(curr as usize)).next;
            }
        }
        unsafe {
            (*self.desc.add(curr as usize)).next = self.free_head;
        }
        self.free_head = head;
        self.free_count += count;
    }

    /// Notify device (ring doorbell).
    pub fn notify(&self, notify_addr: u64) {
        unsafe {
            core::ptr::write_volatile(notify_addr as *mut u16, self.queue_idx);
        }
    }
}

/// VirtIO PCI Device interface.
pub struct VirtioDevice {
    pub vendor_id: u16,
    pub device_id: u16,
    pub pci_bus: u8,
    pub pci_slot: u8,
    pub pci_func: u8,
    pub mem_base: u64,
    pub notify_base: u64,
    pub is_modern: bool,
    pub queues: Vec<VirtQueue>,
    pub features: u64,
}

impl VirtioDevice {
    pub fn new(bus: u8, slot: u8, func: u8) -> Self {
        VirtioDevice {
            vendor_id: 0x1AF4, // Red Hat, Inc.
            device_id: 0,
            pci_bus: bus,
            pci_slot: slot,
            pci_func: func,
            mem_base: 0,
            notify_base: 0,
            is_modern: true,
            queues: Vec::new(),
            features: 0,
        }
    }

    /// Reset the device.
    pub fn reset(&mut self) {
        // Write 0 to device status
        unsafe {
            core::ptr::write_volatile((self.mem_base + 0x14) as *mut u8, 0);
        }
    }

    /// Acknowledge the device.
    pub fn acknowledge(&mut self) {
        unsafe {
            let mut status = core::ptr::read_volatile((self.mem_base + 0x14) as *const u8);
            status |= status::ACKNOWLEDGE | status::DRIVER;
            core::ptr::write_volatile((self.mem_base + 0x14) as *mut u8, status);
        }
    }

    /// Negotiate features.
    pub fn negotiate_features(&mut self, supported: u64) -> bool {
        // Read device features
        let dev_feat = unsafe {
            core::ptr::read_volatile(self.mem_base as *const u32) as u64
        };
        self.features = dev_feat & supported;
        
        // Write guest features
        unsafe {
            core::ptr::write_volatile((self.mem_base + 0x04) as *mut u32, self.features as u32);
        }

        // Set FEATURES_OK
        unsafe {
            let mut status = core::ptr::read_volatile((self.mem_base + 0x14) as *const u8);
            status |= status::FEATURES_OK;
            core::ptr::write_volatile((self.mem_base + 0x14) as *mut u8, status);
            
            // Re-read to verify device accepted features
            let status = core::ptr::read_volatile((self.mem_base + 0x14) as *const u8);
            (status & status::FEATURES_OK) != 0
        }
    }
}
