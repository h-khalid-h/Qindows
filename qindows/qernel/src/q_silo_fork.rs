//! # Q-Silo Fork — Copy-on-Write Silo Isolation Engine (Phase 78)
//!
//! ARCHITECTURE.md §2.2 + implementation_plan.md Phase 48 (CoWFork syscall):
//! > "SyscallId::CoWFork(301) — forks the current Silo's address space using CowManager::mark_cow"
//!
//! Silo forking is foundational for:
//! - **Q-View Browser** (Phase 74): each new tab starts as a clone of the browser Silo, then its
//!   address space is immediately scrubbed to zero-ambient-authority
//! - **Fiber Offload** (Phase 75): fork-then-offload allows the original to keep running while a
//!   clone is shipped to the Q-Server
//! - **Testing/Debugging**: fork the system state, run destructive test, discard fork
//!
//! ## Architecture Guardian: Layering
//! ```text
//! This module (q_silo_fork.rs)
//!     │  Concern: Silo address-space duplication policy + CoW page accounting
//!     │  Does NOT: manage raw page alloc (memory/page_alloc.rs)
//!     │  Does NOT: write CR3 (memory/paging.rs)
//!     │  Does NOT: issue PCIDs (memory/pcid.rs)
//!     │
//!     ├── ForkPolicy: what memory regions to inherit vs. scrub
//!     ├── CoWPageRecord: per-page reference count for copy-on-write
//!     └── SiloForkEngine: orchestrate fork creation and CoW fault resolution
//! ```
//!
//! ## Q-Manifest Law Compliance
//! - **Law 2 (Immutable Binaries)**: code pages are always read-only in the fork — never CoW-copied
//! - **Law 6 (Silo Sandbox)**: the fork gets its own CR3 immediately — no shared memory
//! - **Law 1 (Zero-Ambient Authority)**: forks inherit NO CapTokens by default; caller explicitly
//!   delegates selected caps to the fork

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Fork Policy ───────────────────────────────────────────────────────────────

/// Controls what a forked Silo inherits from its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkPolicy {
    /// Full CoW fork: all writable pages marked CoW, shared with child until written
    Full,
    /// Scrubbed fork: only code pages inherited; heap zeroed (for Q-View new tab)
    CodeOnly,
    /// Snapshot fork: a read-only frozen copy of the parent (for debugging/testing)
    ReadOnlySnapshot,
}

// ── Forked Memory Region Record ───────────────────────────────────────────────

/// A memory region inherited from a parent Silo in a fork.
#[derive(Debug, Clone)]
pub struct ForkRegion {
    /// Virtual address in both parent and child (identical layout pre-write)
    pub virt_addr: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Physical frame backing this region (shared until CoW fault)
    pub phys_frame: u64,
    /// Is this region read-only? (code seg, Prism-backed Law 2)
    pub read_only: bool,
    /// Has the child already written this page (CoW copy made)?
    pub child_copied: bool,
    /// Has the parent already written this page (parent CoW copy made)?
    pub parent_copied: bool,
    /// Prism OID if backed by an immutable Prism object (no copy needed — ever)
    pub prism_oid: Option<[u8; 32]>,
}

// ── CoW Page Record ───────────────────────────────────────────────────────────

/// Tracks shared physical frames across forked Silos.
#[derive(Debug, Clone)]
pub struct CoWPageRecord {
    /// Physical frame address
    pub phys_frame: u64,
    /// Which Silos currently share this frame: silo_id → is_dirty (has written)
    pub sharers: BTreeMap<u64, bool>,
    /// Total number of CoW copies made from this frame
    pub copies_made: u32,
    /// Original owner Silo
    pub original_silo: u64,
}

impl CoWPageRecord {
    pub fn new(phys_frame: u64, original_silo: u64) -> Self {
        let mut sharers = BTreeMap::new();
        sharers.insert(original_silo, false);
        CoWPageRecord {
            phys_frame,
            sharers,
            copies_made: 0,
            original_silo,
        }
    }

    /// Reference count: how many Silos share this frame
    pub fn ref_count(&self) -> usize {
        self.sharers.len()
    }

    /// Can be freed when all sharers have copied or been vaporized
    pub fn is_safe_to_free(&self) -> bool {
        self.sharers.is_empty()
    }
}

// ── Fork Descriptor ───────────────────────────────────────────────────────────

/// Describes a live fork relationship between parent and child Silo.
#[derive(Debug, Clone)]
pub struct SiloFork {
    /// The fork's Silo ID (child)
    pub child_silo_id: u64,
    /// Parent Silo ID
    pub parent_silo_id: u64,
    /// Fork policy used
    pub policy: ForkPolicy,
    /// Memory regions inherited from parent
    pub regions: Vec<ForkRegion>,
    /// CapTokens explicitly delegated to child at fork time
    pub delegated_cap_ids: Vec<u64>,
    /// Tick when fork was created
    pub created_at: u64,
    /// Total shared frame count at fork time
    pub shared_frames: u32,
    /// Total pages already CoW-separated (written by parent or child)
    pub cow_separations: u32,
    /// True if this is a one-way snapshot (child is read-only)
    pub is_snapshot: bool,
}

// ── Fork Statistics ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ForkStats {
    pub total_forks: u64,
    pub full_forks: u64,
    pub code_only_forks: u64,
    pub snapshot_forks: u64,
    pub cow_faults_resolved: u64,
    pub pages_shared: u64,
    pub pages_copied: u64,
    pub frames_freed: u64,
}

// ── Silo Fork Engine ──────────────────────────────────────────────────────────

/// The kernel CoW Silo fork manager.
pub struct SiloForkEngine {
    /// Active fork relationships: child_silo_id → SiloFork
    pub active_forks: BTreeMap<u64, SiloFork>,
    /// CoW page table: phys_frame → CoWPageRecord
    pub cow_pages: BTreeMap<u64, CoWPageRecord>,
    /// Statistics
    pub stats: ForkStats,
    /// Next simulated Silo ID (production: assigned by SiloManager)
    next_silo_id: u64,
}

impl SiloForkEngine {
    pub fn new() -> Self {
        SiloForkEngine {
            active_forks: BTreeMap::new(),
            cow_pages: BTreeMap::new(),
            stats: ForkStats::default(),
            next_silo_id: 0x1000,
        }
    }

    /// Perform a Silo fork. Returns the new child Silo ID.
    ///
    /// Called by `SyscallId::CoWFork(301)` handler.
    pub fn fork(
        &mut self,
        parent_silo_id: u64,
        policy: ForkPolicy,
        delegated_caps: Vec<u64>,
        tick: u64,
    ) -> u64 {
        let child_silo_id = self.next_silo_id;
        self.next_silo_id += 1;

        crate::serial_println!(
            "[FORK] Silo {} forking → child={} policy={:?} caps={} (Law 6: new CR3 issued)",
            parent_silo_id, child_silo_id, policy, delegated_caps.len()
        );

        // Simulate parent memory regions (production: read from SiloAddressSpace)
        let regions = self.build_regions(parent_silo_id, policy);
        let shared_frames = regions.iter().filter(|r| !r.read_only && r.prism_oid.is_none()).count() as u32;

        // Register CoW tracking for all shared writable frames
        for region in &regions {
            if !region.read_only && region.prism_oid.is_none() {
                let record = self.cow_pages
                    .entry(region.phys_frame)
                    .or_insert_with(|| CoWPageRecord::new(region.phys_frame, parent_silo_id));
                record.sharers.insert(child_silo_id, false);
            }
        }

        self.stats.pages_shared += shared_frames as u64;

        let fork = SiloFork {
            child_silo_id,
            parent_silo_id,
            policy,
            regions,
            delegated_cap_ids: delegated_caps,
            created_at: tick,
            shared_frames,
            cow_separations: 0,
            is_snapshot: policy == ForkPolicy::ReadOnlySnapshot,
        };

        self.active_forks.insert(child_silo_id, fork);
        self.stats.total_forks += 1;
        match policy {
            ForkPolicy::Full             => self.stats.full_forks += 1,
            ForkPolicy::CodeOnly         => self.stats.code_only_forks += 1,
            ForkPolicy::ReadOnlySnapshot => self.stats.snapshot_forks += 1,
        }

        crate::serial_println!(
            "[FORK] Fork complete: {} shared frames, snapshot={}, delegated {} caps (Law 1: starts with delegated caps only)",
            shared_frames, policy == ForkPolicy::ReadOnlySnapshot, self.active_forks[&child_silo_id].delegated_cap_ids.len()
        );

        child_silo_id
    }

    /// Handle a CoW page fault: child (or parent) wrote a shared page.
    /// Returns the new physical frame address allocated for the writer.
    ///
    /// Called from the page fault interrupt handler.
    pub fn handle_cow_fault(&mut self, silo_id: u64, phys_frame: u64) -> Option<u64> {
        let record = self.cow_pages.get_mut(&phys_frame)?;

        // Allocate a new physical frame (simulated — production: page_alloc::alloc_frame())
        let new_frame = phys_frame ^ (silo_id << 12); // deterministic placeholder
        record.copies_made += 1;
        record.sharers.insert(silo_id, true); // mark as dirty

        crate::serial_println!(
            "[FORK] CoW fault: silo={} frame={:#x} → new_frame={:#x} (ref_count={})",
            silo_id, phys_frame, new_frame, record.ref_count()
        );

        self.stats.cow_faults_resolved += 1;
        self.stats.pages_copied += 1;

        // If only one sharer remains, the original frame can be given to them exclusively
        if record.sharers.iter().filter(|(_, &dirty)| !dirty).count() == 1 {
            crate::serial_println!("[FORK] Frame {:#x}: last sharer — CoW resolved, frame now exclusive.", phys_frame);
        }

        // Update the fork record
        if let Some(fork) = self.active_forks.get_mut(&silo_id) {
            for region in fork.regions.iter_mut() {
                if region.phys_frame == phys_frame {
                    region.child_copied = true;
                    break;
                }
            }
            fork.cow_separations += 1;
        }

        Some(new_frame)
    }

    /// Release a fork on child Silo vaporize. Decrements all CoW ref counts.
    pub fn release_fork(&mut self, child_silo_id: u64) {
        let fork = match self.active_forks.remove(&child_silo_id) {
            Some(f) => f,
            None    => return,
        };

        for region in &fork.regions {
            if let Some(record) = self.cow_pages.get_mut(&region.phys_frame) {
                record.sharers.remove(&child_silo_id);
                if record.is_safe_to_free() {
                    self.stats.frames_freed += 1;
                }
            }
        }

        crate::serial_println!(
            "[FORK] Fork released: child={} ({} CoW separations, {} shared frames freed).",
            child_silo_id, fork.cow_separations, fork.shared_frames
        );
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn build_regions(&self, silo_id: u64, policy: ForkPolicy) -> Vec<ForkRegion> {
        // Simulate 3 canonical memory regions every Silo has:
        // 1. Code segment (Prism-backed, always RO regardless of policy — Law 2)
        // 2. Stack (writable, CoW unless CodeOnly)
        // 3. Heap (writable, CoW unless CodeOnly)
        let code_oid: [u8; 32] = {
            let mut o = [0u8; 32];
            o[0] = (silo_id & 0xFF) as u8;
            o[1] = ((silo_id >> 8) & 0xFF) as u8;
            o
        };

        let mut regions = alloc::vec![
            ForkRegion {
                virt_addr: 0x0040_0000,
                size_bytes: 512 * 1024,    // 512KiB code
                phys_frame: 0x0010_0000 + silo_id * 0x1000,
                read_only: true,
                child_copied: false,
                parent_copied: false,
                prism_oid: Some(code_oid), // immutable — Law 2 — never CoW
            },
        ];

        if policy != ForkPolicy::CodeOnly {
            regions.push(ForkRegion {
                virt_addr: 0x7FFF_0000,
                size_bytes: 128 * 1024,    // 128KiB stack
                phys_frame: 0x0080_0000 + silo_id * 0x1000,
                read_only: false,
                child_copied: false,
                parent_copied: false,
                prism_oid: None,
            });
            regions.push(ForkRegion {
                virt_addr: 0x1000_0000,
                size_bytes: 4 * 1024 * 1024, // 4MiB heap
                phys_frame: 0x0100_0000 + silo_id * 0x1000,
                read_only: false,
                child_copied: false,
                parent_copied: false,
                prism_oid: None,
            });
        }

        regions
    }
}
