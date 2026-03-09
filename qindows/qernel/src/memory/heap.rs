//! # Kernel Heap Allocator
//!
//! A simple linked-list heap allocator for dynamic kernel allocations.
//! Enables the `alloc` crate (Vec, Box, String) in kernel space.

use super::FrameAllocator;
use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

/// Heap configuration — placed at 16 MiB in identity-mapped physical memory.
/// Well above the kernel image (loaded at 2 MiB) and its BSS/stack.
const HEAP_START: usize = 0x0100_0000; // 16 MiB
const HEAP_SIZE: usize = 4 * 1024 * 1024; // 4 MiB kernel heap

/// A simple linked-list allocator node
struct FreeNode {
    size: usize,
    next: Option<&'static mut FreeNode>,
}

/// Linked-list heap allocator
pub struct LinkedListAllocator {
    head: FreeNode,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: FreeNode { size: 0, next: None },
        }
    }

    /// Initialize the heap with a memory region.
    ///
    /// # Safety
    /// The caller must ensure the memory region is valid and unused.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        let node = start as *mut FreeNode;
        node.write(FreeNode { size, next: None });
        self.head.next = Some(&mut *node);
    }

    /// Allocate a block of memory.
    pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let (size, align) = (layout.size().max(core::mem::size_of::<FreeNode>()),
                              layout.align().max(core::mem::align_of::<FreeNode>()));

        let mut current = &mut self.head;
        while let Some(ref mut region) = current.next {
            let region_addr = *region as *mut FreeNode as usize;
            let alloc_start = align_up(region_addr, align);
            let alloc_end = alloc_start + size;

            if alloc_end <= region_addr + region.size {
                let excess = region_addr + region.size - alloc_end;
                if excess > core::mem::size_of::<FreeNode>() {
                    // Split the block
                    unsafe {
                        let new_node = alloc_end as *mut FreeNode;
                        new_node.write(FreeNode {
                            size: excess,
                            next: region.next.take(),
                        });
                        current.next = Some(&mut *new_node);
                    }
                } else {
                    current.next = region.next.take();
                }
                return alloc_start as *mut u8;
            }
            current = current.next.as_deref_mut().unwrap();
        }
        core::ptr::null_mut() // OOM
    }

    /// Deallocate a previously allocated block.
    pub fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(core::mem::size_of::<FreeNode>());
        unsafe {
            let node = ptr as *mut FreeNode;
            node.write(FreeNode {
                size,
                next: self.head.next.take(),
            });
            self.head.next = Some(&mut *node);
        }
    }
}

/// Global allocator wrapper
struct QAllocator(Mutex<LinkedListAllocator>);

unsafe impl GlobalAlloc for QAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().deallocate(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: QAllocator = QAllocator(Mutex::new(LinkedListAllocator::new()));

/// Initialize the kernel heap.
///
/// Fix #6: logs the actual heap region so boot output confirms correct mapping.
/// On genesis alpha the bootloader identity-maps all physical RAM, so
/// HEAP_START (virtual) == HEAP_START (physical). The FrameAllocator
/// parameter is currently unused but reserved for future virtual-memory mapping.
pub fn init(frame_allocator: &mut FrameAllocator) {
    let _ = frame_allocator; // Reserved for future page-table mapping

    unsafe {
        ALLOCATOR.0.lock().init(HEAP_START, HEAP_SIZE);
    }

    crate::serial_println!(
        "[OK] Kernel heap initialized: {} KiB @ {:#x}..{:#x} (identity-mapped)",
        HEAP_SIZE / 1024,
        HEAP_START,
        HEAP_START + HEAP_SIZE
    );
}

/// Align `addr` upward to alignment `align`.
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
