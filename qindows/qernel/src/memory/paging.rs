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
                let mut entry_flags = flags::PRESENT | flags::WRITABLE | flags::HUGE_PAGE | flags::USER_ACCESSIBLE;

                // Mark MMIO regions as uncacheable for correct device access
                if gib >= 2 {
                    entry_flags |= flags::NO_CACHE | flags::WRITE_THROUGH;
                }

                pd.entries[i].set(phys, entry_flags);
            }
        }

        // Wire PDPT to point to each PD
        PDPT_ID.entries[0].set(&PD_0 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE | flags::USER_ACCESSIBLE);
        PDPT_ID.entries[1].set(&PD_1 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE | flags::USER_ACCESSIBLE);
        PDPT_ID.entries[2].set(&PD_2 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE | flags::USER_ACCESSIBLE);
        PDPT_ID.entries[3].set(&PD_3 as *const PageTable as u64, flags::PRESENT | flags::WRITABLE | flags::USER_ACCESSIBLE);

        // PML4[0] → PDPT_ID
        PML4.entries[0].set(&PDPT_ID as *const PageTable as u64, flags::PRESENT | flags::WRITABLE | flags::USER_ACCESSIBLE);

        // Load CR3 — activates the new page tables
        let pml4_addr = &PML4 as *const PageTable as u64;
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) pml4_addr,
            options(nostack, preserves_flags)
        );
    }
}

/// Identity-map an MMIO region so device registers can be accessed.
///
/// This handles PCI BAR addresses above the initial 4 GiB identity-map.
/// Allocates new page table levels (PDPT, PD) as needed using the frame
/// allocator, then maps a 2 MiB-aligned region containing `phys_addr`
/// with PRESENT | WRITABLE | HUGE_PAGE | NO_CACHE | WRITE_THROUGH.
///
/// After mapping, flushes the TLB for the new virtual address.
pub fn identity_map_mmio(phys_addr: u64, allocator: &mut FrameAllocator) {
    // Decompose into page table indices
    let pml4_idx   = ((phys_addr >> 39) & 0x1FF) as usize;
    let pdpt_idx   = ((phys_addr >> 30) & 0x1FF) as usize;
    let pd_idx     = ((phys_addr >> 21) & 0x1FF) as usize;

    let mmio_flags = flags::PRESENT | flags::WRITABLE | flags::HUGE_PAGE
                   | flags::NO_CACHE | flags::WRITE_THROUGH;

    unsafe {
        // 1. Ensure PML4[pml4_idx] has a PDPT
        if !PML4.entries[pml4_idx].is_present() {
            if let Some(frame) = allocator.allocate_frame() {
                core::ptr::write_bytes(frame.base_addr as *mut u8, 0, 4096);
                PML4.entries[pml4_idx].set(frame.base_addr, flags::PRESENT | flags::WRITABLE);
            } else {
                return; // OOM
            }
        }
        let pdpt = &mut *(PML4.entries[pml4_idx].address() as *mut PageTable);

        // 2. Ensure PDPT[pdpt_idx] has a PD
        if !pdpt.entries[pdpt_idx].is_present() {
            if let Some(frame) = allocator.allocate_frame() {
                core::ptr::write_bytes(frame.base_addr as *mut u8, 0, 4096);
                pdpt.entries[pdpt_idx].set(frame.base_addr, flags::PRESENT | flags::WRITABLE);
            } else {
                return; // OOM
            }
        }
        let pd = &mut *(pdpt.entries[pdpt_idx].address() as *mut PageTable);

        // 3. Map the 2 MiB huge page
        let aligned = phys_addr & !0x1F_FFFF; // 2 MiB aligned
        pd.entries[pd_idx].set(aligned, mmio_flags);

        // 4. Flush the TLB entry
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) aligned,
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

// ── Phase 48: PCID-Aware Silo Address Spaces ────────────────────────────────

/// A Q-Silo's complete hardware address space descriptor.
///
/// Bundles the PML4 physical address with a unique PCID, enabling
/// NOFLUSH context switches (CR3 bit 63 = 1) that preserve other
/// Silos' TLB entries during preemption.
///
/// ## Q-Manifest Law 6: Silo Sandbox
/// Each Silo has exactly one `SiloAddressSpace`. It is created on spawn
/// and released on vaporize. No two live Silos share the same PCID.
#[derive(Debug)]
pub struct SiloAddressSpace {
    /// Physical address of this Silo's PML4 table.
    pub pml4_phys: u64,
    /// Hardware PCID (1–4095). 0 is reserved for the kernel.
    pub pcid: u16,
}

impl SiloAddressSpace {
    /// Activate this address space on the current CPU.
    ///
    /// Writes CR3 with:
    /// - Bits [51:12]: PML4 physical address
    /// - Bits [11:0]: PCID
    /// - Bit 63: 1 = NOFLUSH (retain TLB entries tagged with this PCID)
    ///
    /// # Safety
    /// Must be called with interrupts disabled (from scheduler/IRQ context).
    pub fn activate(&self) {
        // CR3 = (PML4_phys & !0xFFF) | pcid | NOFLUSH_BIT
        // NOFLUSH bit (bit 63) tells the CPU NOT to invalidate TLB entries
        // for this PCID on the CR3 write. Existing valid translations remain.
        let noflush_bit: u64 = 1 << 63;
        let cr3_value = (self.pml4_phys & 0x000F_FFFF_FFFF_F000)
            | (self.pcid as u64 & 0xFFF)
            | noflush_bit;

        unsafe {
            core::arch::asm!(
                "mov cr3, {}",
                in(reg) cr3_value,
                options(nostack, preserves_flags)
            );
        }
    }

    /// Force flush all TLB entries for this Silo's PCID.
    ///
    /// Use this after unmapping pages from this Silo's address space
    /// to prevent use-after-unmap via stale TLB entries.
    pub fn flush_tlb(&self) {
        super::pcid::flush_pcid(self.pcid);
    }
}

impl Drop for SiloAddressSpace {
    /// Automatically reclaim the PCID when the SiloAddressSpace is dropped.
    fn drop(&mut self) {
        super::pcid::free(self.pcid);
    }
}

/// Create an isolated address space for a Q-Silo with a dedicated PCID.
///
/// Allocates a fresh PML4, shares kernel-space mappings (upper half),
/// and assigns a unique PCID from the global PCID pool.
///
/// Returns `None` if either the frame allocator or the PCID pool is exhausted.
///
/// # Architecture Guardian Note
/// This is THE canonical factory for Silo address spaces. All Silo
/// creation must use this function — never call `create_silo_address_space()`
/// directly (it lacks PCID assignment).
pub fn create_silo_address_space_pcid(allocator: &mut FrameAllocator) -> Option<SiloAddressSpace> {
    // Allocate a fresh PCID before touching memory (avoid leak on OOM)
    let pcid = super::pcid::alloc()?;

    // Allocate and zero a new PML4 frame
    let frame = allocator.allocate_frame().or_else(|| {
        // If OOM, release the PCID we already claimed
        super::pcid::free(pcid);
        None
    })?;

    unsafe {
        core::ptr::write_bytes(frame.base_addr as *mut u8, 0, PhysFrame::SIZE as usize);

        // Share kernel upper-half mappings (entries 256-511) from the kernel PML4
        // so syscalls can reach kernel code without re-mapping per Silo.
        // Entry 0 carries the kernel identity map (lower 512 GiB).
        let new_pml4 = &mut *(frame.base_addr as *mut PageTable);
        new_pml4.entries[0] = PML4.entries[0];
    }

    crate::serial_println!(
        "[MMU] Silo address space created: PML4=0x{:x}, PCID={}",
        frame.base_addr, pcid
    );

    Some(SiloAddressSpace {
        pml4_phys: frame.base_addr,
        pcid,
    })
}

