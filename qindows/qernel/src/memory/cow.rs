//! # Hardware Copy-on-Write Manager
//!
//! Implements Q-Manifest Law 2: Immutable Binaries.
//! All Prism Ghost-Writes and Silo forks use CoW instead of eager copying.
//!
//! ## Mechanism
//!
//! 1. **Mark CoW**: Clear the WRITABLE bit in the PTE. Increment a reference
//!    count in the `CowManager`. The physical frame is now shared.
//! 2. **Write Fault**: When a Silo attempts to write to a CoW page, the CPU
//!    triggers a Page Fault (#PF) with error code bit 1 (write). The page
//!    fault handler calls `handle_cow_fault()`.
//! 3. **Fork**: Allocate a fresh physical frame, copy the old content, update
//!    the faulting Silo's PTE to point to the new frame with WRITABLE set.
//!    Decrement the ref count on the original frame.
//!
//! ## Architecture Guardian Note
//! This module owns CoW state exclusively. The VMM, scheduler, and silo code
//! must call into this module — they must NOT directly manipulate PTEs for
//! CoW purposes.

use alloc::collections::BTreeMap;
use crate::memory::{FrameAllocator, PhysFrame};
use crate::memory::vmm::{VirtualMemoryManager, VirtAddr, MapPermissions};

/// Metadata for a shared Copy-on-Write physical frame.
#[derive(Debug, Clone)]
pub struct CowFrame {
    /// Physical address of the shared frame.
    pub phys_addr: u64,
    /// Number of Silos (or mappings) currently referencing this frame.
    pub ref_count: u32,
    /// Silo ID that originally owned this frame before sharing.
    pub origin_silo: u64,
}

/// Global Copy-on-Write manager.
///
/// Maps physical addresses to their CoW metadata. Kept as a `BTreeMap`
/// for O(log n) lookups — CoW faults are infrequent relative to normal
/// page accesses.
pub struct CowManager {
    /// phys_addr → CoW metadata
    frames: BTreeMap<u64, CowFrame>,
    /// Total CoW faults handled (telemetry)
    pub faults_handled: u64,
    /// Total frames freed after CoW fork (telemetry)
    pub frames_freed: u64,
}

impl CowManager {
    pub fn new() -> Self {
        CowManager {
            frames: BTreeMap::new(),
            faults_handled: 0,
            frames_freed: 0,
        }
    }

    /// Mark a virtual page in a Silo's address space as Copy-on-Write.
    ///
    /// Clears the WRITABLE bit in the PTE so the next write triggers a fault.
    /// Registers the physical frame in the CoW tracking table.
    ///
    /// Called by:
    /// - `SyscallId::CoWFork(301)` — Silo fork
    /// - Prism Ghost-Write path — immutable object versioning
    pub fn mark_cow(
        &mut self,
        vmm: &mut VirtualMemoryManager,
        virt: u64,
        phys: u64,
        silo_id: u64,
    ) {
        // Re-map the page as READ-ONLY (clear WRITABLE) without reallocating
        let ro_perms = MapPermissions {
            read: true,
            write: false,      // ← This is the CoW trigger
            execute: false,
            user: true,
            global: false,
        };
        vmm.map_page(VirtAddr(virt), phys, ro_perms);

        // Track or increment ref count
        let entry = self.frames.entry(phys).or_insert(CowFrame {
            phys_addr: phys,
            ref_count: 1,
            origin_silo: silo_id,
        });
        entry.ref_count += 1;
    }

    /// Handle a write page fault on a CoW page.
    ///
    /// Called from `interrupts/page_fault.rs` when:
    /// - Error code bit 1 is set (write fault)
    /// - Fault address is registered in `CowManager`
    ///
    /// Returns `true` if the fault was a CoW fault and has been resolved.
    /// Returns `false` if the address is not a CoW page (genuine segfault).
    pub fn handle_cow_fault(
        &mut self,
        vmm: &mut VirtualMemoryManager,
        allocator: &mut FrameAllocator,
        fault_addr: u64,
        silo_id: u64,
    ) -> bool {
        // Align fault address to 4 KiB page boundary
        let page_addr = fault_addr & !0xFFF;

        // Resolve the virtual address to its physical frame via the VMM's walk
        // (simplified: we look up via the CoW table which stores phys per virt)
        let phys = match self.resolve_phys(vmm, page_addr) {
            Some(p) => p,
            None => return false,
        };

        if !self.frames.contains_key(&phys) {
            return false; // Not a CoW page — genuine fault
        }

        // Allocate a fresh frame for this Silo's private copy
        let new_frame = match allocator.allocate_frame() {
            Some(f) => f,
            None => {
                // OOM — kill the Silo (Sentinel will handle)
                return false;
            }
        };

        // Copy the old page content to the new frame
        unsafe {
            core::ptr::copy_nonoverlapping(
                phys as *const u8,
                new_frame.base_addr as *mut u8,
                4096,
            );
        }

        // Remap this Silo's virtual page to the new private frame (WRITABLE)
        let rw_perms = MapPermissions {
            read: true,
            write: true,
            execute: false,
            user: true,
            global: false,
        };
        vmm.map_page(VirtAddr(page_addr), new_frame.base_addr, rw_perms);

        // Decrement ref count on the old shared frame
        if let Some(cow_frame) = self.frames.get_mut(&phys) {
            cow_frame.ref_count = cow_frame.ref_count.saturating_sub(1);
            if cow_frame.ref_count == 0 {
                // Last reference released — free the original frame
                let frame = PhysFrame { base_addr: phys };
                allocator.deallocate_frame(frame);
                self.frames.remove(&phys);
                self.frames_freed += 1;
            }
        }

        self.faults_handled += 1;
        crate::serial_println!(
            "[CoW] Silo {} forked page 0x{:x} → new frame 0x{:x}",
            silo_id, page_addr, new_frame.base_addr
        );
        true
    }

    /// Query if a physical address is currently tracked as CoW.
    pub fn is_cow_frame(&self, phys: u64) -> bool {
        self.frames.contains_key(&phys)
    }

    /// Get the reference count for a physical frame.
    pub fn ref_count(&self, phys: u64) -> u32 {
        self.frames.get(&phys).map(|f| f.ref_count).unwrap_or(0)
    }

    /// Total number of tracked CoW frames.
    pub fn tracked_frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Walk the VMM's page table to resolve a virtual address → physical frame.
    ///
    /// This is a read-only walk; it does not allocate.
    fn resolve_phys(&self, vmm: &VirtualMemoryManager, virt: u64) -> Option<u64> {
        let vaddr = VirtAddr(virt);
        unsafe {
            let pml4 = &*(vmm.pml4_phys as *const crate::memory::vmm::PageTable);
            let pml4e = &pml4.entries[vaddr.pml4_index()];
            if !pml4e.is_present() { return None; }

            let pdpt = &*(pml4e.phys_addr() as *const crate::memory::vmm::PageTable);
            let pdpte = &pdpt.entries[vaddr.pdpt_index()];
            if !pdpte.is_present() { return None; }

            let pd = &*(pdpte.phys_addr() as *const crate::memory::vmm::PageTable);
            let pde = &pd.entries[vaddr.pd_index()];
            if !pde.is_present() { return None; }
            if pde.is_huge() {
                // 2 MiB huge page
                return Some(pde.phys_addr() + (virt & 0x1F_FFFF));
            }

            let pt = &*(pde.phys_addr() as *const crate::memory::vmm::PageTable);
            let pte = &pt.entries[vaddr.pt_index()];
            if !pte.is_present() { return None; }
            Some(pte.phys_addr())
        }
    }
}
