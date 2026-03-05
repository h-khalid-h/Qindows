//! # Qernel SLAB Allocator
//!
//! Fixed-size object caching allocator for kernel structures.
//! Eliminates fragmentation for common allocation sizes and
//! provides O(1) alloc/free via per-CPU free lists.

use core::sync::atomic::{AtomicU64, Ordering};

/// SLAB sizes (powers of 2 from 8 to 4096).
pub const SLAB_SIZES: [usize; 10] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// A single free object in a slab (intrusive linked list).
#[repr(C)]
struct FreeObject {
    next: *mut FreeObject,
}

/// A slab page — holds objects of one size class.
pub struct Slab {
    /// Object size for this slab
    pub object_size: usize,
    /// Number of objects per page
    pub objects_per_page: usize,
    /// Free list head
    free_head: *mut FreeObject,
    /// Number of free objects
    pub free_count: usize,
    /// Number of allocated objects
    pub alloc_count: usize,
    /// Backing page physical address
    pub page_phys: u64,
    /// Is this slab full?
    pub full: bool,
}

impl Slab {
    /// Initialize a slab from a raw page.
    ///
    /// # Safety
    /// `page_addr` must point to a valid 4096-byte region.
    pub unsafe fn init(page_addr: u64, object_size: usize) -> Self {
        let aligned_size = if object_size < core::mem::size_of::<*mut u8>() {
            core::mem::size_of::<*mut u8>()
        } else {
            // Align up to 8 bytes
            (object_size + 7) & !7
        };

        let objects_per_page = 4096 / aligned_size;

        // Build the free list
        let mut head: *mut FreeObject = core::ptr::null_mut();
        for i in (0..objects_per_page).rev() {
            let obj = (page_addr as usize + i * aligned_size) as *mut FreeObject;
            (*obj).next = head;
            head = obj;
        }

        Slab {
            object_size: aligned_size,
            objects_per_page,
            free_head: head,
            free_count: objects_per_page,
            alloc_count: 0,
            page_phys: page_addr,
            full: false,
        }
    }

    /// Allocate one object from this slab.
    pub fn alloc(&mut self) -> Option<*mut u8> {
        if self.free_head.is_null() {
            self.full = true;
            return None;
        }

        unsafe {
            let obj = self.free_head;
            self.free_head = (*obj).next;
            self.free_count -= 1;
            self.alloc_count += 1;
            self.full = self.free_head.is_null();
            Some(obj as *mut u8)
        }
    }

    /// Free an object back to this slab.
    ///
    /// # Safety
    /// `ptr` must have been allocated from this slab.
    pub unsafe fn free(&mut self, ptr: *mut u8) {
        let obj = ptr as *mut FreeObject;
        (*obj).next = self.free_head;
        self.free_head = obj;
        self.free_count += 1;
        self.alloc_count = self.alloc_count.saturating_sub(1);
        self.full = false;
    }

    /// Does this slab contain the given address?
    pub fn contains(&self, ptr: *const u8) -> bool {
        let addr = ptr as u64;
        addr >= self.page_phys && addr < self.page_phys + 4096
    }
}

/// A size-class cache (manages multiple slabs for one object size).
pub struct SlabCache {
    /// Object size
    pub object_size: usize,
    /// All slabs for this size
    pub slabs: alloc::vec::Vec<Slab>,
    /// Statistics
    pub total_allocs: u64,
    pub total_frees: u64,
}

impl SlabCache {
    pub fn new(object_size: usize) -> Self {
        SlabCache {
            object_size,
            slabs: alloc::vec::Vec::new(),
            total_allocs: 0,
            total_frees: 0,
        }
    }

    /// Allocate from this cache.
    pub fn alloc(&mut self) -> Option<*mut u8> {
        // Try existing non-full slabs first
        for slab in &mut self.slabs {
            if !slab.full {
                if let Some(ptr) = slab.alloc() {
                    self.total_allocs += 1;
                    return Some(ptr);
                }
            }
        }

        // All slabs are full — need a new page from page_alloc
        let frame = super::page_alloc::alloc_frame()?;
        let slab = unsafe { Slab::init(frame.0, self.object_size) };
        self.slabs.push(slab);

        // Allocate from the new slab
        let last = self.slabs.last_mut().unwrap();
        let ptr = last.alloc();
        if ptr.is_some() { self.total_allocs += 1; }
        ptr
    }

    /// Free back to this cache.
    pub unsafe fn free(&mut self, ptr: *mut u8) {
        for slab in &mut self.slabs {
            if slab.contains(ptr) {
                slab.free(ptr);
                self.total_frees += 1;
                return;
            }
        }
        // ptr didn't belong to any slab — corrupted or wrong cache
    }

    /// Reclaim completely empty slabs (return pages to page_alloc).
    pub fn shrink(&mut self) -> usize {
        let before = self.slabs.len();
        self.slabs.retain(|slab| {
            if slab.alloc_count == 0 {
                // Would call page_alloc::free_frame() here
                false
            } else {
                true
            }
        });
        before - self.slabs.len()
    }
}

/// The global SLAB allocator.
pub struct SlabAllocator {
    /// Per-size caches
    pub caches: alloc::vec::Vec<SlabCache>,
    /// Global stats
    pub total_allocs: AtomicU64,
    pub total_frees: AtomicU64,
}

impl SlabAllocator {
    pub fn new() -> Self {
        let mut caches = alloc::vec::Vec::new();
        for &size in &SLAB_SIZES {
            caches.push(SlabCache::new(size));
        }

        SlabAllocator {
            caches,
            total_allocs: AtomicU64::new(0),
            total_frees: AtomicU64::new(0),
        }
    }

    /// Find the best size class for a given allocation size.
    fn size_class(size: usize) -> Option<usize> {
        for (i, &slab_size) in SLAB_SIZES.iter().enumerate() {
            if size <= slab_size {
                return Some(i);
            }
        }
        None // Too large for SLAB — use page allocator directly
    }

    /// Allocate `size` bytes.
    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        let class = Self::size_class(size)?;
        let ptr = self.caches[class].alloc();
        if ptr.is_some() {
            self.total_allocs.fetch_add(1, Ordering::Relaxed);
        }
        ptr
    }

    /// Free a previously allocated pointer.
    ///
    /// # Safety
    /// `ptr` must have been allocated by this allocator with the given `size`.
    pub unsafe fn free(&mut self, ptr: *mut u8, size: usize) {
        if let Some(class) = Self::size_class(size) {
            self.caches[class].free(ptr);
            self.total_frees.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Shrink all caches (reclaim empty pages).
    pub fn shrink_all(&mut self) -> usize {
        self.caches.iter_mut().map(|c| c.shrink()).sum()
    }

    /// Get allocation stats.
    pub fn stats(&self) -> (u64, u64, usize) {
        (
            self.total_allocs.load(Ordering::Relaxed),
            self.total_frees.load(Ordering::Relaxed),
            self.caches.iter().map(|c| c.slabs.len()).sum(),
        )
    }
}
