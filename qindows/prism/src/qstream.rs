//! # Prism Q-Stream — Zero-Copy Memory-Mapped I/O
//!
//! The Q-Stream layer replaces the traditional Open→Read→Close loop
//! with memory-mapped persistence. Files map directly into virtual
//! memory, and the NVMe handles data flow via DMA, bypassing the
//! CPU entirely (Section 3.2 of the Qindows Spec).
//!
//! Key features:
//! - **Memory-mapped persistence**: Reading a file = reading a variable
//! - **Ghost-Write**: Atomic versioning via CoW — writes go to new
//!   NVMe blocks, old versions become Shadow Objects
//! - **Lazy loading**: Only pages actually accessed are faulted in
//! - **Unified buffer cache**: Single pool shared between FS and apps

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Stream access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamMode {
    /// Read-only mapping (pages are CoW-protected)
    ReadOnly,
    /// Read-write (writes create Ghost-Write versions)
    ReadWrite,
    /// Append-only (sequential writes)
    Append,
    /// Direct I/O (bypass buffer cache, DMA only)
    Direct,
}

/// Page state in the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageState {
    /// Not yet faulted in (lazy)
    Unmapped,
    /// Clean (matches on-disk copy)
    Clean,
    /// Dirty (modified, pending Ghost-Write)
    Dirty,
    /// Being written back to storage
    Writeback,
    /// Pinned in memory (won't be evicted)
    Pinned,
}

/// A single mapped page.
#[derive(Debug, Clone)]
pub struct MappedPage {
    /// Virtual address of this page
    pub virt_addr: u64,
    /// Physical frame backing this page
    pub phys_frame: u64,
    /// Offset within the object (in 4 KiB pages)
    pub page_offset: u64,
    /// State
    pub state: PageState,
    /// Access count (for eviction decisions)
    pub access_count: u64,
    /// Last access timestamp
    pub last_access: u64,
    /// Is this a Ghost-Write (CoW) page?
    pub is_ghost: bool,
}

/// A Q-Stream mapping — maps a Prism Object into virtual memory.
#[derive(Debug, Clone)]
pub struct QStream {
    /// Stream ID
    pub id: u64,
    /// Prism Object ID being mapped
    pub oid: u64,
    /// Access mode
    pub mode: StreamMode,
    /// Base virtual address of the mapping
    pub base_addr: u64,
    /// Total size of the object (bytes)
    pub size: u64,
    /// Mapped pages
    pub pages: BTreeMap<u64, MappedPage>,
    /// Silo owning this stream
    pub silo_id: u64,
    /// Object version (for Ghost-Write tracking)
    pub version: u64,
    /// Shadow Object versions (previous Ghost-Writes)
    pub shadow_versions: Vec<u64>,
    /// Has pending dirty pages?
    pub has_dirty: bool,
}

impl QStream {
    /// Access a byte within the stream (triggers page fault if unmapped).
    pub fn access(&mut self, offset: u64, now: u64) -> Result<u64, &'static str> {
        if offset >= self.size {
            return Err("Access beyond stream bounds");
        }

        let page_offset = offset / 4096;
        let page = self.pages.entry(page_offset).or_insert_with(|| {
            // Lazy page fault — map this page
            MappedPage {
                virt_addr: self.base_addr + page_offset * 4096,
                phys_frame: 0, // Would be allocated by VMM
                page_offset,
                state: PageState::Clean,
                access_count: 0,
                last_access: now,
                is_ghost: false,
            }
        });

        page.access_count = page.access_count.saturating_add(1);
        page.last_access = now;

        Ok(page.virt_addr + (offset % 4096))
    }

    /// Write to the stream (Ghost-Write: creates a CoW page).
    pub fn write(&mut self, offset: u64, _data: &[u8], now: u64) -> Result<(), &'static str> {
        if self.mode == StreamMode::ReadOnly {
            return Err("Stream is read-only");
        }
        if offset >= self.size && self.mode != StreamMode::Append {
            return Err("Write beyond stream bounds");
        }

        let page_offset = offset / 4096;
        let page = self.pages.entry(page_offset).or_insert_with(|| {
            MappedPage {
                virt_addr: self.base_addr + page_offset * 4096,
                phys_frame: 0,
                page_offset,
                state: PageState::Unmapped,
                access_count: 0,
                last_access: now,
                is_ghost: false,
            }
        });

        // Ghost-Write: mark as dirty, will be written to a new block
        if page.state == PageState::Clean || page.state == PageState::Unmapped {
            page.is_ghost = true;
        }
        page.state = PageState::Dirty;
        page.last_access = now;
        self.has_dirty = true;

        Ok(())
    }

    /// Flush dirty pages (commit Ghost-Writes to storage).
    pub fn flush(&mut self) -> usize {
        let mut flushed = 0;
        for page in self.pages.values_mut() {
            if page.state == PageState::Dirty {
                page.state = PageState::Writeback;
                // In production: DMA the page to a new NVMe block
                page.state = PageState::Clean;
                page.is_ghost = false;
                flushed += 1;
            }
        }

        if flushed > 0 {
            self.shadow_versions.push(self.version);
            self.version += 1;
        }
        self.has_dirty = false;
        flushed
    }

    /// Pin a page range in memory (prevent eviction).
    pub fn pin_range(&mut self, start_page: u64, count: u64) {
        for off in start_page..start_page + count {
            if let Some(page) = self.pages.get_mut(&off) {
                page.state = PageState::Pinned;
            }
        }
    }

    /// Count mapped pages.
    pub fn mapped_pages(&self) -> usize {
        self.pages.len()
    }

    /// Count dirty pages.
    pub fn dirty_pages(&self) -> usize {
        self.pages.values().filter(|p| p.state == PageState::Dirty).count()
    }
}

/// Q-Stream Manager — manages all active streams.
pub struct QStreamManager {
    /// Active streams by ID
    pub streams: BTreeMap<u64, QStream>,
    /// Next stream ID
    next_id: u64,
    /// Next virtual address base for mappings
    next_vaddr: u64,
    /// Statistics
    pub stats: QStreamStats,
}

/// Q-Stream statistics.
#[derive(Debug, Clone, Default)]
pub struct QStreamStats {
    pub streams_opened: u64,
    pub streams_closed: u64,
    pub page_faults: u64,
    pub ghost_writes: u64,
    pub flushes: u64,
    pub total_pages_mapped: u64,
    pub bytes_dma_transferred: u64,
}

impl QStreamManager {
    pub fn new() -> Self {
        QStreamManager {
            streams: BTreeMap::new(),
            next_id: 1,
            // User-space mappings start at this address
            next_vaddr: 0x0000_7000_0000_0000,
            stats: QStreamStats::default(),
        }
    }

    /// Open a Q-Stream to a Prism Object.
    pub fn open(
        &mut self,
        oid: u64,
        size: u64,
        mode: StreamMode,
        silo_id: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let base = self.next_vaddr;
        let pages_needed = (size + 4095) / 4096;
        self.next_vaddr += pages_needed * 4096;

        self.streams.insert(id, QStream {
            id,
            oid,
            mode,
            base_addr: base,
            size,
            pages: BTreeMap::new(),
            silo_id,
            version: 1,
            shadow_versions: Vec::new(),
            has_dirty: false,
        });

        self.stats.streams_opened += 1;
        id
    }

    /// Close a stream (flush and release).
    pub fn close(&mut self, stream_id: u64) -> Result<(), &'static str> {
        let stream = self.streams.get_mut(&stream_id)
            .ok_or("Stream not found")?;

        // Auto-flush dirty pages
        let flushed = stream.flush();
        if flushed > 0 {
            self.stats.flushes += 1;
            self.stats.ghost_writes += flushed as u64;
        }

        self.streams.remove(&stream_id);
        self.stats.streams_closed += 1;
        Ok(())
    }

    /// Get a mutable stream reference.
    pub fn get_mut(&mut self, stream_id: u64) -> Option<&mut QStream> {
        self.streams.get_mut(&stream_id)
    }

    /// Rollback an object to a previous version (Shadow Object).
    pub fn rollback(&mut self, stream_id: u64) -> Result<u64, &'static str> {
        let stream = self.streams.get_mut(&stream_id)
            .ok_or("Stream not found")?;

        let prev_version = stream.shadow_versions.pop()
            .ok_or("No shadow versions available")?;

        stream.version = prev_version;
        // In production: remap pages to the shadow version's blocks
        // and discard the current version
        stream.pages.clear();
        stream.has_dirty = false;

        Ok(prev_version)
    }

    /// Evict cold pages across all streams (LRU-based).
    pub fn evict_cold_pages(&mut self, max_to_evict: usize, now: u64, age_threshold: u64) -> usize {
        let mut evicted = 0;

        for stream in self.streams.values_mut() {
            let cold_keys: Vec<u64> = stream.pages.iter()
                .filter(|(_, p)| {
                    p.state == PageState::Clean
                        && now.saturating_sub(p.last_access) > age_threshold
                })
                .map(|(&k, _)| k)
                .collect();

            for key in cold_keys {
                stream.pages.remove(&key);
                evicted += 1;
                if evicted >= max_to_evict {
                    return evicted;
                }
            }
        }

        evicted
    }
}
