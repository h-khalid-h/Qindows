//! # Qernel Live-Patching / Hot-Swap Engine
//!
//! Enables **Atomic Hot-Swap** — applying kernel and driver updates
//! without rebooting. Key to the "No Reboot" promise in the Qindows
//! architecture (Section 12 of the spec).
//!
//! How it works:
//! 1. Load the new binary (Wasm/native) into a staging area
//! 2. Verify its cryptographic signature against The Ledger
//! 3. Freeze all Fibers touching the target module
//! 4. Swap function pointers atomically (memory fence)
//! 5. Resume Fibers — they call into the new code
//! 6. Garbage-collect the old binary after drain timeout

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Patch target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchTarget {
    /// A kernel subsystem (e.g., scheduler, memory manager)
    KernelModule,
    /// A user-mode driver
    Driver,
    /// A system service (e.g., Prism, Aether, Nexus)
    Service,
    /// A Sentinel enforcement rule
    SentinelRule,
}

/// Patch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchState {
    /// Staged (loaded into memory, not yet applied)
    Staged,
    /// Signature verified
    Verified,
    /// Fibers frozen, ready to swap
    FibersQuiesced,
    /// Swapped — new code is live
    Applied,
    /// Rolled back to previous version
    RolledBack,
    /// Failed (verification or apply error)
    Failed,
}

/// A hot-swap patch.
#[derive(Debug, Clone)]
pub struct Patch {
    /// Patch ID
    pub id: u64,
    /// Target module name
    pub module_name: String,
    /// Target type
    pub target: PatchTarget,
    /// Current state
    pub state: PatchState,
    /// Old binary hash (what we're replacing)
    pub old_hash: [u8; 32],
    /// New binary hash (what we're applying)
    pub new_hash: [u8; 32],
    /// New binary size in bytes
    pub new_size: u64,
    /// Staging address (where the new binary is loaded)
    pub staging_addr: u64,
    /// Old entry point (for rollback)
    pub old_entry_point: u64,
    /// New entry point
    pub new_entry_point: u64,
    /// Applied timestamp
    pub applied_at: Option<u64>,
    /// Fibers that were quiesced during swap
    pub quiesced_fibers: Vec<u64>,
}

/// Rollback information.
#[derive(Debug, Clone)]
pub struct RollbackEntry {
    pub patch_id: u64,
    pub module_name: String,
    pub old_entry_point: u64,
    pub old_hash: [u8; 32],
    pub rolled_back_at: u64,
}

/// Hot-swap statistics.
#[derive(Debug, Clone, Default)]
pub struct HotSwapStats {
    pub patches_staged: u64,
    pub patches_applied: u64,
    pub patches_failed: u64,
    pub patches_rolled_back: u64,
    pub total_downtime_us: u64, // Total microseconds of quiescence
    pub fibers_quiesced: u64,
}

/// Next patch ID counter.
static NEXT_PATCH_ID: AtomicU64 = AtomicU64::new(1);

/// The Hot-Swap Engine.
pub struct HotSwapEngine {
    /// Staged patches
    pub patches: BTreeMap<u64, Patch>,
    /// Module → current entry point mapping
    pub module_entries: BTreeMap<String, u64>,
    /// Rollback history (last N swaps)
    pub rollback_log: Vec<RollbackEntry>,
    /// Maximum rollback entries to keep
    pub max_rollback_entries: usize,
    /// Statistics
    pub stats: HotSwapStats,
}

impl HotSwapEngine {
    pub fn new() -> Self {
        HotSwapEngine {
            patches: BTreeMap::new(),
            module_entries: BTreeMap::new(),
            rollback_log: Vec::new(),
            max_rollback_entries: 64,
            stats: HotSwapStats::default(),
        }
    }

    /// Register a kernel module with its current entry point.
    pub fn register_module(&mut self, name: &str, entry_point: u64, hash: [u8; 32]) {
        self.module_entries.insert(String::from(name), entry_point);
    }

    /// Stage a new patch.
    pub fn stage_patch(
        &mut self,
        module_name: &str,
        new_hash: [u8; 32],
        new_size: u64,
        staging_addr: u64,
        new_entry_point: u64,
    ) -> u64 {
        let id = NEXT_PATCH_ID.fetch_add(1, Ordering::Relaxed);

        let old_entry = self.module_entries.get(module_name).copied().unwrap_or(0);

        self.patches.insert(id, Patch {
            id,
            module_name: String::from(module_name),
            target: PatchTarget::KernelModule,
            state: PatchState::Staged,
            old_hash: [0; 32], // Would come from module registry
            new_hash,
            new_size,
            staging_addr,
            old_entry_point: old_entry,
            new_entry_point,
            applied_at: None,
            quiesced_fibers: Vec::new(),
        });

        self.stats.patches_staged += 1;
        id
    }

    /// Verify the patch signature against The Ledger.
    pub fn verify_patch(&mut self, patch_id: u64) -> Result<(), &'static str> {
        let patch = self.patches.get_mut(&patch_id)
            .ok_or("Patch not found")?;

        if patch.state != PatchState::Staged {
            return Err("Patch not in staged state");
        }

        // In production: verify Ed25519 signature of new_hash
        // against The Ledger's trusted key list
        if patch.new_hash == [0; 32] {
            patch.state = PatchState::Failed;
            self.stats.patches_failed += 1;
            return Err("Invalid patch hash");
        }

        patch.state = PatchState::Verified;
        Ok(())
    }

    /// Apply a verified patch (quiesce → swap → resume).
    pub fn apply_patch(&mut self, patch_id: u64, now: u64) -> Result<(), &'static str> {
        let patch = self.patches.get_mut(&patch_id)
            .ok_or("Patch not found")?;

        if patch.state != PatchState::Verified {
            return Err("Patch not verified");
        }

        // Step 1: Quiesce all fibers using this module
        // In production: freeze fibers via scheduler, wait for safe-points
        patch.state = PatchState::FibersQuiesced;

        // Step 2: Atomic swap — update the module entry point
        // Memory fence ensures all cores see the new pointer
        core::sync::atomic::fence(Ordering::SeqCst);

        let module_name = patch.module_name.clone();
        let new_entry = patch.new_entry_point;
        let old_entry = patch.old_entry_point;

        self.module_entries.insert(module_name.clone(), new_entry);
        patch.state = PatchState::Applied;
        patch.applied_at = Some(now);

        // Step 3: Resume fibers
        self.stats.patches_applied += 1;

        // Step 4: Record rollback info
        self.rollback_log.push(RollbackEntry {
            patch_id,
            module_name,
            old_entry_point: old_entry,
            old_hash: patch.old_hash,
            rolled_back_at: 0,
        });

        // Trim rollback log
        while self.rollback_log.len() > self.max_rollback_entries {
            self.rollback_log.remove(0);
        }

        Ok(())
    }

    /// Rollback the last patch to a module.
    pub fn rollback(&mut self, module_name: &str, now: u64) -> Result<(), &'static str> {
        let entry = self.rollback_log.iter_mut()
            .rev()
            .find(|e| e.module_name == module_name && e.rolled_back_at == 0)
            .ok_or("No rollback entry for this module")?;

        // Restore old entry point
        self.module_entries.insert(
            entry.module_name.clone(),
            entry.old_entry_point,
        );
        entry.rolled_back_at = now;

        // Mark the patch as rolled back
        if let Some(patch) = self.patches.get_mut(&entry.patch_id) {
            patch.state = PatchState::RolledBack;
        }

        self.stats.patches_rolled_back += 1;
        Ok(())
    }

    /// Get the current entry point for a module.
    pub fn get_entry_point(&self, module_name: &str) -> Option<u64> {
        self.module_entries.get(module_name).copied()
    }

    /// List all patches.
    pub fn list_patches(&self) -> Vec<(u64, &str, PatchState)> {
        self.patches.values()
            .map(|p| (p.id, p.module_name.as_str(), p.state))
            .collect()
    }
}
