//! # Four-Level Page Table Manager (Gap 18.5)
//!
//! Implements x86-64 4-level paging:
//!   PML4 → PDPT → PD → PT → 4 KiB physical frame
//!
//! Each level is a 4 KiB array of 512 × 8-byte entries (Page Table Entries).
//! This module provides `map_page` and `unmap_page` using `frame_alloc` to
//! dynamically allocate intermediate table frames — no static page tables needed.
//!
//! ## Entry Flags
//! - Bit 0: PRESENT — must be 1 for valid entries
//! - Bit 1: WRITE — read-write if set, read-only if clear
//! - Bit 2: USER — accessible from Ring-3 if set
//! - Bit 3: WRITE_THROUGH — write-through caching
//! - Bit 4: CACHE_DISABLE — disables caching (MMIO regions)
//! - Bit 5: ACCESSED — set by CPU on first access
//! - Bit 6: DIRTY — set by CPU on write (PT-level only)
//! - Bit 63: NO_EXECUTE — prevent instruction fetch (NX bit)
//!
//! ## Safety
//! - `map_page` and `unmap_page` are `unsafe` — caller owns the page table root
//!   and must ensure no aliasing with live CPU translations
//! - All physical addresses are flat-mapped in Genesis Alpha (phys == virt)

use crate::frame_alloc;

// ── PTE Flag Constants ────────────────────────────────────────────────────────

pub const PAGE_PRESENT:       u64 = 1 << 0;
pub const PAGE_WRITE:         u64 = 1 << 1;
pub const PAGE_USER:          u64 = 1 << 2;
pub const PAGE_WRITE_THROUGH: u64 = 1 << 3;
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4;
pub const PAGE_NX:            u64 = 1 << 63;

/// Default flags for a normal kernel R/W page.
pub const FLAGS_KERNEL_RW: u64 = PAGE_PRESENT | PAGE_WRITE;
/// Default flags for a Ring-3 R/W page.
pub const FLAGS_USER_RW:   u64 = PAGE_PRESENT | PAGE_WRITE | PAGE_USER;
/// Flags for MMIO regions (uncached, no-execute).
pub const FLAGS_MMIO:      u64 = PAGE_PRESENT | PAGE_WRITE | PAGE_CACHE_DISABLE | PAGE_NX;

// ── Page Table Entry helpers ──────────────────────────────────────────────────

/// Extract the physical address from a PTE (bits 12–51).
#[inline]
fn pte_phys(pte: u64) -> u64 {
    pte & 0x000F_FFFF_FFFF_F000
}

/// Build a non-leaf (directory) PTE pointing at a child table.
#[inline]
fn dir_pte(child_phys: u64) -> u64 {
    // Present + Write + User (to allow child leaf to control access)
    (child_phys & 0x000F_FFFF_FFFF_F000) | PAGE_PRESENT | PAGE_WRITE | PAGE_USER
}

// ── Index extraction from a 64-bit virtual address ───────────────────────────

#[inline] fn pml4_idx(va: u64) -> usize { ((va >> 39) & 0x1FF) as usize }
#[inline] fn pdpt_idx(va: u64) -> usize { ((va >> 30) & 0x1FF) as usize }
#[inline] fn pd_idx  (va: u64) -> usize { ((va >> 21) & 0x1FF) as usize }
#[inline] fn pt_idx  (va: u64) -> usize { ((va >> 12) & 0x1FF) as usize }

// ── Table pointer helpers ─────────────────────────────────────────────────────

/// Return a &mut [u64; 512] pointing at a 4 KiB page-table page.
/// In Genesis Alpha all physical addresses are identity-mapped (phys == virt).
#[inline]
unsafe fn table_at(phys: u64) -> &'static mut [u64; 512] {
    &mut *(phys as *mut [u64; 512])
}

/// Allocate a fresh zeroed 4 KiB page table frame.
/// Panics (via unwrap) if the frame allocator is exhausted — should never happen at boot.
fn alloc_table() -> u64 {
    let frame = frame_alloc::alloc()
        .expect("[PAGE_TABLE] frame_alloc exhausted — cannot allocate page table");
    let phys = frame_alloc::frame_to_phys(frame);
    // Zero the page (entries = not-present)
    unsafe {
        core::ptr::write_bytes(phys as *mut u8, 0, 4096);
    }
    phys
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Gap 18.5 — Map a single 4 KiB virtual page to a physical frame.
///
/// Walks the 4-level PML4 hierarchy rooted at `page_table_root` (physical base
/// of the PML4 table). Intermediate directory frames are allocated from
/// `frame_alloc` on demand. The leaf PTE is written with `phys | flags`.
///
/// # Safety
/// - `page_table_root` must point to a valid, exclusively-owned PML4 frame
/// - `virt` must be 4 KiB aligned  
/// - No TLB flush is performed; caller is responsible for `invlpg` or CR3 reload
pub unsafe fn map_page(page_table_root: u64, virt: u64, phys: u64, flags: u64) {
    debug_assert_eq!(virt & 0xFFF, 0, "map_page: virt must be 4KiB-aligned");
    debug_assert_eq!(phys & 0xFFF, 0, "map_page: phys must be 4KiB-aligned");

    // ── Level 4: PML4 ────────────────────────────────────────────────────────
    let pml4 = table_at(page_table_root);
    let l4e  = &mut pml4[pml4_idx(virt)];
    if *l4e & PAGE_PRESENT == 0 {
        *l4e = dir_pte(alloc_table());
    }

    // ── Level 3: PDPT ────────────────────────────────────────────────────────
    let pdpt = table_at(pte_phys(*l4e));
    let l3e  = &mut pdpt[pdpt_idx(virt)];
    if *l3e & PAGE_PRESENT == 0 {
        *l3e = dir_pte(alloc_table());
    }

    // ── Level 2: PD ──────────────────────────────────────────────────────────
    let pd   = table_at(pte_phys(*l3e));
    let l2e  = &mut pd[pd_idx(virt)];
    if *l2e & PAGE_PRESENT == 0 {
        *l2e = dir_pte(alloc_table());
    }

    // ── Level 1: PT (leaf) ───────────────────────────────────────────────────
    let pt  = table_at(pte_phys(*l2e));
    let l1e = &mut pt[pt_idx(virt)];
    *l1e = (phys & 0x000F_FFFF_FFFF_F000) | (flags & !0x000F_FFFF_FFFF_F000);
}

/// Unmap a single 4 KiB virtual page.
///
/// Clears the leaf PTE to 0 (not-present). Intermediate directory frames are
/// **not** freed — they may have other live mappings.
///
/// Performs `invlpg` to invalidate the TLB entry for `virt`.
///
/// # Safety
/// - `page_table_root` must be the root of the currently active page table
///   (or the caller must have exclusive access to it)
pub unsafe fn unmap_page(page_table_root: u64, virt: u64) {
    let pml4 = table_at(page_table_root);
    let l4e  = pml4[pml4_idx(virt)];
    if l4e & PAGE_PRESENT == 0 { return; }

    let pdpt = table_at(pte_phys(l4e));
    let l3e  = pdpt[pdpt_idx(virt)];
    if l3e & PAGE_PRESENT == 0 { return; }

    let pd   = table_at(pte_phys(l3e));
    let l2e  = pd[pd_idx(virt)];
    if l2e & PAGE_PRESENT == 0 { return; }

    let pt   = table_at(pte_phys(l2e));
    pt[pt_idx(virt)] = 0;

    // Invalidate TLB for this virtual address
    core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

/// Check whether a virtual address is mapped in the given page table.
pub unsafe fn is_mapped(page_table_root: u64, virt: u64) -> bool {
    let pml4 = table_at(page_table_root);
    let l4e  = pml4[pml4_idx(virt)];
    if l4e & PAGE_PRESENT == 0 { return false; }
    let pdpt = table_at(pte_phys(l4e));
    let l3e  = pdpt[pdpt_idx(virt)];
    if l3e & PAGE_PRESENT == 0 { return false; }
    let pd   = table_at(pte_phys(l3e));
    let l2e  = pd[pd_idx(virt)];
    if l2e & PAGE_PRESENT == 0 { return false; }
    let pt   = table_at(pte_phys(l2e));
    pt[pt_idx(virt)] & PAGE_PRESENT != 0
}
