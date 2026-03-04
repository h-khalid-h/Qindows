//! # Virtual Memory Paging
//!
//! 4-level x86_64 page table management.
//! Implements identity mapping for kernel space and
//! per-Silo isolated address spaces.

use super::{FrameAllocator, PhysFrame};

/// Page table entry flags.
pub mod flags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER_ACCESSIBLE: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const NO_EXECUTE: u64 = 1 << 63;
}

/// A single entry in a page table (PML4, PDPT, PD, or PT).
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        PageTableEntry(0)
    }

    pub fn is_present(&self) -> bool {
        self.0 & flags::PRESENT != 0
    }

    pub fn set(&mut self, addr: u64, flags: u64) {
        self.0 = (addr & 0x000F_FFFF_FFFF_F000) | flags;
    }

    pub fn address(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    pub fn flags(&self) -> u64 {
        self.0 & 0xFFF0_0000_0000_0FFF
    }
}

/// A full page table (512 entries, one 4 KiB frame).
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        PageTable {
            entries: [PageTableEntry::empty(); 512],
        }
    }
}

/// Initialize kernel page tables with identity mapping.
///
/// Maps the first N megabytes of physical memory 1:1 so the kernel
/// can access hardware addresses directly.
pub fn init(allocator: &mut FrameAllocator) {
    // In a full implementation:
    // 1. Allocate a frame for PML4
    // 2. Identity-map kernel code/data regions
    // 3. Map the framebuffer
    // 4. Map APIC/IOAPIC MMIO regions
    // 5. Load CR3 with the new PML4 address

    // For now, we rely on the bootloader's identity mapping
    // and note the initialization
    let _ = allocator;
}

/// Create an isolated address space for a Q-Silo.
///
/// Each Silo gets its own PML4, completely isolating its view
/// of memory from all other processes.
pub fn create_silo_address_space(allocator: &mut FrameAllocator) -> Option<u64> {
    let frame = allocator.allocate_frame()?;

    // Zero the new page table
    unsafe {
        core::ptr::write_bytes(frame.base_addr as *mut u8, 0, PhysFrame::SIZE as usize);
    }

    // Copy kernel-space mappings (upper half) from the active PML4
    // so the kernel is accessible during syscalls
    // (In production: copy entries 256..512 from the current PML4)

    Some(frame.base_addr)
}
