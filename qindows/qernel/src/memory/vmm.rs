//! # Qernel Virtual Memory Mapper
//!
//! 4-level page table management (PML4 → PDPT → PD → PT).
//! Maps virtual addresses to physical frames with fine-grained
//! permissions. Supports demand paging, guard pages, and ASLR.


/// Page table entry flags.
pub mod flags {
    pub const PRESENT: u64     = 1 << 0;
    pub const WRITABLE: u64    = 1 << 1;
    pub const USER: u64        = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const CACHE_DISABLE: u64 = 1 << 4;
    pub const ACCESSED: u64    = 1 << 5;
    pub const DIRTY: u64       = 1 << 6;
    pub const HUGE_PAGE: u64   = 1 << 7;
    pub const GLOBAL: u64      = 1 << 8;
    pub const NO_EXECUTE: u64  = 1 << 63;
}

/// Virtual address (48-bit with sign extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// PML4 index (bits 39-47).
    pub fn pml4_index(&self) -> usize { ((self.0 >> 39) & 0x1FF) as usize }
    /// PDPT index (bits 30-38).
    pub fn pdpt_index(&self) -> usize { ((self.0 >> 30) & 0x1FF) as usize }
    /// PD index (bits 21-29).
    pub fn pd_index(&self) -> usize { ((self.0 >> 21) & 0x1FF) as usize }
    /// PT index (bits 12-20).
    pub fn pt_index(&self) -> usize { ((self.0 >> 12) & 0x1FF) as usize }
    /// Page offset (bits 0-11).
    pub fn page_offset(&self) -> usize { (self.0 & 0xFFF) as usize }
    /// Is this a canonical address?
    pub fn is_canonical(&self) -> bool {
        let bits47 = (self.0 >> 47) & 1;
        if bits47 == 0 { self.0 < (1u64 << 47) }
        else { self.0 >= 0xFFFF_8000_0000_0000 }
    }
}

/// A page table entry (8 bytes, one of 512 per table).
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub fn new() -> Self { PageTableEntry(0) }

    pub fn is_present(&self) -> bool { self.0 & flags::PRESENT != 0 }
    pub fn is_writable(&self) -> bool { self.0 & flags::WRITABLE != 0 }
    pub fn is_user(&self) -> bool { self.0 & flags::USER != 0 }
    pub fn is_huge(&self) -> bool { self.0 & flags::HUGE_PAGE != 0 }
    pub fn is_executable(&self) -> bool { self.0 & flags::NO_EXECUTE == 0 }

    /// Get the physical address this entry points to.
    pub fn phys_addr(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    /// Set the entry to map to a physical address with given flags.
    pub fn set(&mut self, phys_addr: u64, entry_flags: u64) {
        self.0 = (phys_addr & 0x000F_FFFF_FFFF_F000) | entry_flags;
    }

    /// Clear the entry.
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// A page table (512 entries = 4 KiB).
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub fn new() -> Self {
        PageTable {
            entries: [PageTableEntry::new(); 512],
        }
    }
}

/// Memory mapping permissions.
#[derive(Debug, Clone, Copy)]
pub struct MapPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
    pub global: bool,
}

impl MapPermissions {
    pub fn kernel_rw() -> Self {
        MapPermissions { read: true, write: true, execute: false, user: false, global: true }
    }
    pub fn kernel_rx() -> Self {
        MapPermissions { read: true, write: false, execute: true, user: false, global: true }
    }
    pub fn user_rw() -> Self {
        MapPermissions { read: true, write: true, execute: false, user: true, global: false }
    }
    pub fn user_rx() -> Self {
        MapPermissions { read: true, write: false, execute: true, user: true, global: false }
    }
    pub fn user_rwx() -> Self {
        MapPermissions { read: true, write: true, execute: true, user: true, global: false }
    }

    /// Convert to page table entry flags.
    pub fn to_flags(&self) -> u64 {
        let mut f = flags::PRESENT;
        if self.write { f |= flags::WRITABLE; }
        if self.user { f |= flags::USER; }
        if !self.execute { f |= flags::NO_EXECUTE; }
        if self.global { f |= flags::GLOBAL; }
        f
    }
}

/// A virtual memory mapping region.
#[derive(Debug, Clone)]
pub struct VmRegion {
    /// Start virtual address
    pub start: u64,
    /// Size in bytes
    pub size: u64,
    /// Permissions
    pub permissions: MapPermissions,
    /// Region type
    pub region_type: VmRegionType,
    /// Number of pages mapped
    pub pages_mapped: u64,
}

/// Virtual memory region types.
#[derive(Debug, Clone, Copy)]
pub enum VmRegionType {
    /// Kernel code
    KernelCode,
    /// Kernel data / BSS
    KernelData,
    /// Kernel heap
    KernelHeap,
    /// Kernel stack
    KernelStack,
    /// User code
    UserCode,
    /// User data
    UserData,
    /// User heap
    UserHeap,
    /// User stack
    UserStack,
    /// Memory-mapped file
    MemoryMapped,
    /// Guard page (unmapped, triggers fault)
    Guard,
    /// MMIO (device registers)
    Mmio,
}

/// The Virtual Memory Manager.
pub struct VirtualMemoryManager {
    /// PML4 physical address
    pub pml4_phys: u64,
    /// Tracked regions
    pub regions: alloc::vec::Vec<VmRegion>,
    /// Pages currently mapped
    pub pages_mapped: u64,
    /// TLB flushes performed
    pub tlb_flushes: u64,
}

impl VirtualMemoryManager {
    pub fn new(pml4_phys: u64) -> Self {
        VirtualMemoryManager {
            pml4_phys,
            regions: alloc::vec::Vec::new(),
            pages_mapped: 0,
            tlb_flushes: 0,
        }
    }

    /// Map a single page.
    pub fn map_page(&mut self, virt: VirtAddr, phys: u64, perms: MapPermissions) {
        let entry_flags = perms.to_flags();

        unsafe {
            // Walk / create page tables
            let pml4 = &mut *(self.pml4_phys as *mut PageTable);
            let pdpt = self.ensure_table(&mut pml4.entries[virt.pml4_index()], entry_flags);
            let pd = self.ensure_table(&mut (*pdpt).entries[virt.pdpt_index()], entry_flags);
            let pt = self.ensure_table(&mut (*pd).entries[virt.pd_index()], entry_flags);

            (*pt).entries[virt.pt_index()].set(phys, entry_flags);
        }

        self.pages_mapped += 1;
        self.flush_tlb_page(virt.0);
    }

    /// Unmap a single page.
    pub fn unmap_page(&mut self, virt: VirtAddr) {
        unsafe {
            let pml4 = &mut *(self.pml4_phys as *mut PageTable);
            if !pml4.entries[virt.pml4_index()].is_present() { return; }

            let pdpt = pml4.entries[virt.pml4_index()].phys_addr() as *mut PageTable;
            if !(*pdpt).entries[virt.pdpt_index()].is_present() { return; }

            let pd = (*pdpt).entries[virt.pdpt_index()].phys_addr() as *mut PageTable;
            if !(*pd).entries[virt.pd_index()].is_present() { return; }

            let pt = (*pd).entries[virt.pd_index()].phys_addr() as *mut PageTable;
            (*pt).entries[virt.pt_index()].clear();
        }

        self.pages_mapped = self.pages_mapped.saturating_sub(1);
        self.flush_tlb_page(virt.0);
    }

    /// Map a contiguous range of pages.
    pub fn map_range(
        &mut self,
        virt_start: u64,
        phys_start: u64,
        page_count: u64,
        perms: MapPermissions,
    ) {
        for i in 0..page_count {
            let virt = VirtAddr(virt_start + i * 4096);
            let phys = phys_start + i * 4096;
            self.map_page(virt, phys, perms);
        }

        self.regions.push(VmRegion {
            start: virt_start,
            size: page_count * 4096,
            permissions: perms,
            region_type: VmRegionType::UserData,
            pages_mapped: page_count,
        });
    }

    /// Flush TLB for a single page.
    fn flush_tlb_page(&mut self, addr: u64) {
        unsafe { core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)); }
        self.tlb_flushes += 1;
    }

    /// Flush the entire TLB (reload CR3).
    pub fn flush_tlb_all(&mut self) {
        unsafe {
            let cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
            core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
        }
        self.tlb_flushes += 1;
    }

    /// Ensure a page table exists at a PTE, allocating if needed.
    unsafe fn ensure_table(&self, entry: &mut PageTableEntry, parent_flags: u64) -> *mut PageTable {
        if entry.is_present() {
            entry.phys_addr() as *mut PageTable
        } else {
            // Allocate a physical frame for the new page table
            let frame = super::page_alloc::alloc_frame()
                .expect("VMM: out of physical memory for page table");
            let frame_phys = frame.0;
            let table = frame_phys as *mut PageTable;
            // Zero the table
            core::ptr::write_bytes(table, 0, 1);
            entry.set(frame_phys, parent_flags | flags::PRESENT | flags::WRITABLE);
            table
        }
    }
}
