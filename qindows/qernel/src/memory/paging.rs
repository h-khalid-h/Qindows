//! # Virtual Memory Paging
//!
//! 4-level x86_64 page table management.
//! Identity-maps the first 4 GiB of physical address space,
//! covering kernel code, heap, framebuffer, and all MMIO regions.
//! Implements per-Silo isolated address spaces.

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

/// Static page table storage in BSS.
///
/// Identity-map the first 4 GiB using 2 MiB huge pages.
///
/// PML4[0] → PDPT_ID
/// PDPT_ID[0] → PD_0  (0x0000_0000 – 0x3FFF_FFFF: kernel, heap, low RAM)
/// PDPT_ID[1] → PD_1  (0x4000_0000 – 0x7FFF_FFFF: mid RAM)
/// PDPT_ID[2] → PD_2  (0x8000_0000 – 0xBFFF_FFFF: framebuffer)
/// PDPT_ID[3] → PD_3  (0xC000_0000 – 0xFFFF_FFFF: APIC, IOAPIC, HPET)
static mut PML4: PageTable = PageTable::new();
static mut PDPT_ID: PageTable = PageTable::new();
static mut PD_0: PageTable = PageTable::new();
static mut PD_1: PageTable = PageTable::new();
static mut PD_2: PageTable = PageTable::new();
static mut PD_3: PageTable = PageTable::new();

/// Initialize kernel page tables.
///
/// Identity-maps the first 4 GiB of physical address space using
/// 2 MiB huge pages, covering:
/// - Kernel code & BSS at 2 MiB
/// - Kernel heap at 16 MiB
/// - Framebuffer at ~0x8000_0000 (2 GiB)
/// - Local APIC at 0xFEE0_0000
/// - IO APIC at 0xFEC0_0000
/// - HPET at 0xFED0_0000
///
/// Loads CR3 with the new PML4 to activate the mapping.
pub fn init(allocator: &mut FrameAllocator) {
    let _ = allocator; // Frames not needed — we use 2 MiB huge pages

    unsafe {
        // Fill each Page Directory with 512 × 2 MiB huge pages
        // for a total of 4 × 1 GiB = 4 GiB identity-mapped.
        let pds: [*mut PageTable; 4] = [
            &mut PD_0 as *mut PageTable,
            &mut PD_1 as *mut PageTable,
            &mut PD_2 as *mut PageTable,
            &mut PD_3 as *mut PageTable,
        ];

        for (gib, pd_ptr) in pds.iter().enumerate() {
            let pd = &mut **pd_ptr;
            for i in 0..512 {
                let phys = (gib as u64) * 0x4000_0000 + (i as u64) * 0x20_0000;
                let mut entry_flags = flags::PRESENT | flags::WRITABLE | flags::HUGE_PAGE;

                // Mark MMIO regions as uncacheable for correct device access
                if gib >= 2 {
                    entry_flags |= flags::NO_CACHE | flags::WRITE_THROUGH;
                }

                pd.entries[i].set(phys, entry_flags);
            }
        }

        // Wire PDPT to point to each PD
        PDPT_ID.entries[0].set(&PD_0 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE);
        PDPT_ID.entries[1].set(&PD_1 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE);
        PDPT_ID.entries[2].set(&PD_2 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE);
        PDPT_ID.entries[3].set(&PD_3 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE);

        // PML4[0] → PDPT_ID
        PML4.entries[0].set(&PDPT_ID as *const PageTable as u64, flags::PRESENT | flags::WRITABLE);

        // Load CR3 — activates the new page tables
        let pml4_addr = &PML4 as *const PageTable as u64;
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) pml4_addr,
            options(nostack, preserves_flags)
        );
    }
}

/// Create an isolated address space for a Q-Silo.
///
/// Each Silo gets its own PML4, completely isolating its view
/// of memory from all other processes. Kernel-space mappings
/// are shared so syscalls can access kernel code.
pub fn create_silo_address_space(allocator: &mut FrameAllocator) -> Option<u64> {
    let frame = allocator.allocate_frame()?;

    // Zero the new PML4
    unsafe {
        core::ptr::write_bytes(frame.base_addr as *mut u8, 0, PhysFrame::SIZE as usize);

        // Copy kernel-space mappings from the active PML4
        // Entry 0 (first 512 GiB) contains the kernel identity mapping
        let new_pml4 = &mut *(frame.base_addr as *mut PageTable);
        new_pml4.entries[0] = PML4.entries[0];
    }

    Some(frame.base_addr)
}
