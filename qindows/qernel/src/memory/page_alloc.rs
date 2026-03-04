//! # Qernel Page Frame Allocator
//!
//! Physical page frame allocator using a bitmap.
//! Manages all physical memory, tracks free/used frames,
//! and provides allocation for the virtual memory subsystem.

use core::sync::atomic::{AtomicU64, Ordering};

/// Page size: 4 KiB.
pub const PAGE_SIZE: usize = 4096;

/// Maximum physical memory supported: 64 GiB.
const MAX_PHYS_MEM: u64 = 64 * 1024 * 1024 * 1024;

/// Maximum number of page frames.
const MAX_FRAMES: usize = (MAX_PHYS_MEM / PAGE_SIZE as u64) as usize;

/// Bitmap words (each u64 tracks 64 pages).
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

/// The bitmap — 1 = used, 0 = free.
static mut BITMAP: [u64; BITMAP_WORDS] = [0; BITMAP_WORDS];

/// Total frames available.
static TOTAL_FRAMES: AtomicU64 = AtomicU64::new(0);
/// Free frames remaining.
static FREE_FRAMES: AtomicU64 = AtomicU64::new(0);
/// Next hint (for fast allocation).
static NEXT_FREE_HINT: AtomicU64 = AtomicU64::new(0);

/// A physical page frame address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysFrame(pub u64);

impl PhysFrame {
    /// Get the frame number from the address.
    pub fn number(&self) -> usize {
        (self.0 / PAGE_SIZE as u64) as usize
    }

    /// Get the physical address from a frame number.
    pub fn from_number(n: usize) -> Self {
        PhysFrame(n as u64 * PAGE_SIZE as u64)
    }

    /// Is this frame address aligned?
    pub fn is_aligned(&self) -> bool {
        self.0 % PAGE_SIZE as u64 == 0
    }
}

/// Memory region descriptor (from bootloader).
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    /// Start physical address
    pub start: u64,
    /// Length in bytes
    pub length: u64,
    /// Region type
    pub region_type: MemoryRegionType,
}

/// Memory region types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Usable RAM
    Usable,
    /// Reserved (BIOS, MMIO)
    Reserved,
    /// ACPI reclaimable
    AcpiReclaimable,
    /// ACPI NVS
    AcpiNvs,
    /// Bad memory
    BadMemory,
    /// Kernel code/data
    KernelAndModules,
    /// Bootloader data
    BootloaderReclaimable,
}

/// Initialize the page frame allocator from the memory map.
pub fn init(regions: &[MemoryRegion]) {
    let mut total = 0u64;
    let mut free = 0u64;

    unsafe {
        // Mark everything as used initially
        for word in BITMAP.iter_mut() {
            *word = u64::MAX;
        }

        // Free usable regions
        for region in regions {
            if region.region_type != MemoryRegionType::Usable { continue; }

            let start_frame = (region.start as usize + PAGE_SIZE - 1) / PAGE_SIZE; // Round up
            let end_frame = ((region.start + region.length) as usize) / PAGE_SIZE; // Round down

            for frame in start_frame..end_frame {
                if frame < MAX_FRAMES {
                    // Clear the bit (mark as free)
                    let word = frame / 64;
                    let bit = frame % 64;
                    BITMAP[word] &= !(1u64 << bit);
                    free += 1;
                }
            }
            total += (end_frame - start_frame) as u64;
        }

        // Always mark frame 0 as used (null guard)
        BITMAP[0] |= 1;
        if free > 0 { free -= 1; }
    }

    TOTAL_FRAMES.store(total, Ordering::Relaxed);
    FREE_FRAMES.store(free, Ordering::Relaxed);

    crate::serial_println!(
        "[OK] Page allocator: {} MiB usable ({} frames, {} free)",
        total * PAGE_SIZE as u64 / (1024 * 1024),
        total,
        free
    );
}

/// Allocate a single physical page frame.
pub fn alloc_frame() -> Option<PhysFrame> {
    unsafe {
        let hint = NEXT_FREE_HINT.load(Ordering::Relaxed) as usize;

        // Search from hint
        for i in 0..BITMAP_WORDS {
            let idx = (hint / 64 + i) % BITMAP_WORDS;
            let word = BITMAP[idx];

            if word == u64::MAX { continue; } // All used

            // Find first zero bit
            let bit = (!word).trailing_zeros() as usize;
            if bit >= 64 { continue; }

            let frame = idx * 64 + bit;
            if frame >= MAX_FRAMES { continue; }

            // Mark as used
            BITMAP[idx] |= 1u64 << bit;
            FREE_FRAMES.fetch_sub(1, Ordering::Relaxed);
            NEXT_FREE_HINT.store(frame as u64 + 1, Ordering::Relaxed);

            return Some(PhysFrame::from_number(frame));
        }

        None // Out of memory
    }
}

/// Allocate N contiguous physical frames.
pub fn alloc_contiguous(count: usize) -> Option<PhysFrame> {
    if count == 0 { return None; }
    if count == 1 { return alloc_frame(); }

    unsafe {
        // Linear scan for contiguous free run
        let mut run_start = 0;
        let mut run_len = 0;

        for frame in 0..MAX_FRAMES {
            let word = frame / 64;
            let bit = frame % 64;

            if BITMAP[word] & (1u64 << bit) == 0 {
                // Frame is free
                if run_len == 0 { run_start = frame; }
                run_len += 1;

                if run_len >= count {
                    // Found! Mark all as used
                    for f in run_start..run_start + count {
                        let w = f / 64;
                        let b = f % 64;
                        BITMAP[w] |= 1u64 << b;
                    }
                    FREE_FRAMES.fetch_sub(count as u64, Ordering::Relaxed);
                    return Some(PhysFrame::from_number(run_start));
                }
            } else {
                run_len = 0;
            }
        }

        None
    }
}

/// Free a single physical page frame.
pub fn free_frame(frame: PhysFrame) {
    let n = frame.number();
    if n == 0 || n >= MAX_FRAMES { return; } // Don't free null or out of range

    unsafe {
        let word = n / 64;
        let bit = n % 64;

        if BITMAP[word] & (1u64 << bit) != 0 {
            BITMAP[word] &= !(1u64 << bit);
            FREE_FRAMES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Free N contiguous frames.
pub fn free_contiguous(start: PhysFrame, count: usize) {
    for i in 0..count {
        free_frame(PhysFrame::from_number(start.number() + i));
    }
}

/// Get the number of free frames.
pub fn free_count() -> u64 {
    FREE_FRAMES.load(Ordering::Relaxed)
}

/// Get total usable frames.
pub fn total_count() -> u64 {
    TOTAL_FRAMES.load(Ordering::Relaxed)
}

/// Get free memory in bytes.
pub fn free_bytes() -> u64 {
    free_count() * PAGE_SIZE as u64
}

/// Get usage percentage.
pub fn usage_percent() -> f32 {
    let total = total_count();
    if total == 0 { return 0.0; }
    let used = total - free_count();
    (used as f32 / total as f32) * 100.0
}
