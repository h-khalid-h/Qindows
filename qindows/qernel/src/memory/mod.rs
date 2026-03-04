//! # Qernel Memory Management
//!
//! The "Object-Space" allocator — manages physical frames, virtual paging,
//! and the kernel heap. Every allocation produces a Capability Token.

pub mod heap;
pub mod page_alloc;
pub mod paging;

use spin::Mutex;

/// A physical memory frame (4 KiB page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysFrame {
    pub base_addr: u64,
}

impl PhysFrame {
    pub const SIZE: u64 = 4096;

    pub fn containing_address(addr: u64) -> Self {
        PhysFrame {
            base_addr: addr & !(Self::SIZE - 1),
        }
    }
}

/// Physical frame allocator using a bitmap.
///
/// Tracks which 4 KiB frames are free/used across all physical RAM.
/// The bitmap is stored in a statically-allocated region after the kernel.
pub struct FrameAllocator {
    /// Bitmap: 1 bit per 4 KiB frame. 1 = used, 0 = free.
    bitmap: &'static mut [u8],
    /// Total number of frames in the system
    total_frames: usize,
    /// Next frame to check (optimization hint)
    next_free: usize,
}

impl FrameAllocator {
    /// Initialize from the UEFI memory map.
    ///
    /// Scans the memory map to determine usable regions and builds
    /// the allocation bitmap.
    pub fn init(
        memory_map_addr: u64,
        memory_map_entries: u64,
        desc_size: u64,
    ) -> Self {
        // In a full implementation, we would:
        // 1. Parse the UEFI MemoryDescriptor array
        // 2. Find the largest CONVENTIONAL memory region
        // 3. Place the bitmap at the start of that region
        // 4. Mark kernel/bootloader/firmware regions as used

        // Placeholder: assume 256 MB of usable RAM for the bitmap
        let total_frames = (256 * 1024 * 1024) / PhysFrame::SIZE as usize;
        let bitmap_size = total_frames / 8;

        // Safety: this would point to a real memory region in production
        let bitmap = unsafe {
            core::slice::from_raw_parts_mut(
                memory_map_addr as *mut u8,
                bitmap_size,
            )
        };

        // Mark all frames as free initially
        bitmap.fill(0);

        FrameAllocator {
            bitmap,
            total_frames,
            next_free: 0,
        }
    }

    /// Allocate a single physical frame.
    ///
    /// Returns the frame address, or None if OOM.
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        for i in self.next_free..self.total_frames {
            let byte_idx = i / 8;
            let bit_idx = i % 8;

            if self.bitmap[byte_idx] & (1 << bit_idx) == 0 {
                // Mark as used
                self.bitmap[byte_idx] |= 1 << bit_idx;
                self.next_free = i + 1;
                return Some(PhysFrame {
                    base_addr: (i as u64) * PhysFrame::SIZE,
                });
            }
        }
        None
    }

    /// Free a previously allocated frame.
    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        let idx = (frame.base_addr / PhysFrame::SIZE) as usize;
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;
        self.bitmap[byte_idx] &= !(1 << bit_idx);

        // Update hint for faster next allocation
        if idx < self.next_free {
            self.next_free = idx;
        }
    }

    /// Returns the total number of free frames.
    pub fn free_frame_count(&self) -> usize {
        let mut count = 0;
        for i in 0..self.total_frames {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if self.bitmap[byte_idx] & (1 << bit_idx) == 0 {
                count += 1;
            }
        }
        count
    }
}

/// Global frame allocator, protected by a spinlock.
pub static FRAME_ALLOCATOR: Mutex<Option<FrameAllocator>> = Mutex::new(None);
