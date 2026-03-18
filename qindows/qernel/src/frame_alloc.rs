//! # Physical Frame Allocator (Gap 17.2)
//!
//! O(1) lock-free bitmap allocator for 4 KiB physical frames.
//!
//! ## Layout
//! - 64 × AtomicU64 words = 4096 frame bits
//! - Each bit = one 4 KiB physical frame
//! - Frame N is at physical address N × 4096
//! - Total addressable RAM: 4096 × 4 KiB = **16 MiB**
//!   (enough for initial boot; Phase 18 extends to contiguous multi-word scan)
//!
//! ## Concurrency
//! - `alloc()` uses CAS (compare-and-swap) loop on the bitmap word — lock-free
//! - `free()` uses atomic OR — lock-free
//! - `init()` runs single-threaded at boot before any alloc calls
//!
//! ## Integration
//! - Called from `boot_sequence::boot_phase2()` via `frame_alloc::init(&memory_map)`
//! - `handle_alloc_frames()` in `syscall/mod.rs` calls `frame_alloc::alloc()`
//! - `handle_free_frames()` calls `frame_alloc::free(frame_phys)`

use core::sync::atomic::{AtomicU64, Ordering};

/// Number of 4 KiB frames the allocator tracks (64 bitmap words × 64 bits).
pub const TOTAL_FRAMES: usize = 4096;

/// Bitmap: bit=1 means frame is FREE.
/// Initialized to 0 (all reserved); `init()` marks conventional memory frames as free.
static BITMAP: [AtomicU64; 64] = {
    const Z: AtomicU64 = AtomicU64::new(0);
    [Z; 64]
};

/// Number of free frames remaining.
static FREE_COUNT: AtomicU64 = AtomicU64::new(0);

/// EFI memory map entry (simplified — only what we need).
/// Bootloader fills this and passes it to the kernel in `BootInfo`.
#[derive(Clone, Copy, Debug)]
pub struct MemEntry {
    /// EFI memory type (7 = EfiConventionalMemory, the only usable type).
    pub mem_type: u32,
    /// Physical start address of this region.
    pub phys_start: u64,
    /// Number of 4 KiB pages in this region.
    pub page_count: u64,
}

/// Gap 17.2 — Seed the frame bitmap from the EFI memory map.
///
/// Called once from `boot_sequence::boot_phase2()` after the EFI memory map
/// has been parsed. Marks every `EfiConventionalMemory` (type 7) frame as free.
///
/// Frames below 1 MiB (0x00100000) are skipped — they contain the real-mode IVT,
/// BIOS data area, and legacy video buffer.
pub fn init(map: &[MemEntry]) {
    let mut seeded: u64 = 0;
    for entry in map {
        if entry.mem_type != 7 { continue; } // Only EfiConventionalMemory
        let start_page = (entry.phys_start >> 12) as usize;
        let count = entry.page_count as usize;
        for i in 0..count {
            let frame = start_page + i;
            if frame < 256 || frame >= TOTAL_FRAMES { continue; } // skip <1MiB, skip overflow
            mark_free(frame);
            seeded += 1;
        }
    }
    FREE_COUNT.store(seeded, Ordering::Relaxed);
    crate::serial_println!(
        "[FRAME_ALLOC] Physical memory map seeded: {} frames available ({} KiB free)",
        seeded,
        seeded * 4
    );
}

/// Mark a frame as free (bit = 1).
fn mark_free(frame: usize) {
    let word = frame / 64;
    let bit = frame % 64;
    BITMAP[word].fetch_or(1u64 << bit, Ordering::Relaxed);
}

/// Allocate one physical 4 KiB frame. Returns the frame number, or None if exhausted.
///
/// Algorithm: scan bitmap words for any nonzero word (has free frame), then
/// use `tzcnt` (trailing-zero-count = index of lowest free bit) and CAS to claim it.
pub fn alloc() -> Option<u64> {
    for (word_idx, word_atom) in BITMAP.iter().enumerate() {
        // Fast-reject: if word is 0, no free frames in this 64-frame group
        let mut current = word_atom.load(Ordering::Relaxed);
        while current != 0 {
            // Find lowest free frame bit (trailing zero count = position of lowest set bit)
            let bit = current.trailing_zeros() as usize;
            let mask = 1u64 << bit;
            // CAS: claim the bit (0 it out)
            match word_atom.compare_exchange_weak(
                current, current & !mask,
                Ordering::AcqRel, Ordering::Relaxed,
            ) {
                Ok(_) => {
                    FREE_COUNT.fetch_sub(1, Ordering::Relaxed);
                    let frame = (word_idx * 64 + bit) as u64;
                    return Some(frame);
                }
                Err(actual) => {
                    // Another CPU claimed this bit — retry with fresh value
                    current = actual;
                }
            }
        }
    }
    None // Out of frames
}

/// Free a physical frame (by frame number, not byte address).
///
/// # Safety
/// Caller must ensure `frame` was previously allocated and is no longer mapped
/// into any page table. Double-free is detectable (bit already 1) but not fatal.
pub fn free(frame: u64) {
    let frame = frame as usize;
    if frame >= TOTAL_FRAMES { return; }
    let word = frame / 64;
    let bit = frame % 64;
    let mask = 1u64 << bit;
    let prev = BITMAP[word].fetch_or(mask, Ordering::Release);
    if prev & mask == 0 {
        // Was actually allocated — count it as a real free
        FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    // If bit was already 1, it's a double-free — log but don't panic in kernel
}

/// Return the number of free frames remaining.
pub fn free_count() -> u64 {
    FREE_COUNT.load(Ordering::Relaxed)
}

/// Return the byte address of a frame number.
#[inline]
pub fn frame_to_phys(frame: u64) -> u64 {
    frame << 12 // × 4096
}
