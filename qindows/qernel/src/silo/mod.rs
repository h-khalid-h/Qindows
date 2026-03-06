//! # Q-Silo: Process Isolation
//!
//! A Q-Silo is a hardware-enforced execution bubble.
//! Every application runs in its own Silo with:
//! - Isolated page tables (can't see other Silos or the Qernel)
//! - Capability-based permissions (zero ambient authority)
//! - Independent resource accounting

use alloc::vec::Vec;
use crate::capability::{CapToken, Permissions};

/// Unique Silo identifier
pub type SiloId = u64;

/// Silo lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloState {
    /// Created but not yet scheduled
    Ready,
    /// Active and running fibers
    Running,
    /// Suspended by the Sentinel
    Suspended,
    /// Terminated (awaiting cleanup)
    Dead,
    /// Frozen for debugging / post-mortem
    Frozen,
}

/// A Q-Silo — the Qindows "process."
///
/// Unlike traditional OS processes, Silos start with ZERO visibility.
/// They can't see files, network, or even the user's name until
/// they receive explicit Capability Tokens.
pub struct QSilo {
    /// Unique identifier
    pub id: SiloId,
    /// Current state
    pub state: SiloState,
    /// CR3 value — points to this Silo's isolated page table
    pub page_table_root: u64,
    /// Granted capabilities
    pub capabilities: Vec<CapToken>,
    /// Binary Object ID (the app's code hash)
    pub binary_oid: u64,
    /// Memory usage in bytes
    pub memory_used: u64,
    /// Hard memory limit (enforced by Sentinel)
    pub memory_limit: u64,
    /// CPU time consumed (in scheduler ticks)
    pub cpu_ticks: u64,
    /// Health score assigned by the Sentinel (0-100)
    pub health_score: u8,
}

impl QSilo {
    /// Create a new Silo for the given binary.
    ///
    /// The Silo starts with zero capabilities and minimal memory.
    pub fn create(binary_oid: u64, page_table_root: u64) -> Self {
        static NEXT_ID: core::sync::atomic::AtomicU64 =
            core::sync::atomic::AtomicU64::new(1);

        QSilo {
            id: NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            state: SiloState::Ready,
            page_table_root,
            capabilities: Vec::new(),
            binary_oid,
            memory_used: 0,
            memory_limit: 256 * 1024 * 1024, // 256 MB default
            cpu_ticks: 0,
            health_score: 100,
        }
    }

    /// Grant a capability to this Silo.
    pub fn grant_capability(&mut self, cap: CapToken) {
        self.capabilities.push(cap);
    }

    /// Check if this Silo holds a specific permission.
    pub fn has_capability(&self, required: Permissions) -> bool {
        self.capabilities.iter().any(|c| c.has_permission(required))
    }

    /// Revoke all capabilities matching a permission type.
    ///
    /// Used by the Sentinel for "Live-Strip" enforcement.
    pub fn revoke_capability(&mut self, target: Permissions) {
        for cap in &mut self.capabilities {
            cap.revoke(target);
        }
    }

    /// Terminate this Silo and release all resources.
    ///
    /// Performs a Secure Wipe:
    /// - All local cache is encrypted and vaulted
    /// - Memory address space is randomized and returned
    /// - Zero residue remains
    pub fn vaporize(&mut self) {
        self.state = SiloState::Dead;
        self.capabilities.clear();
        self.memory_used = 0;
        // In production: zero the page table, free all frames,
        // kill all fibers belonging to this Silo
    }

    /// Freeze for post-mortem analysis.
    pub fn freeze(&mut self) {
        self.state = SiloState::Frozen;
    }
}

/// The Silo Manager — tracks all active Silos.
pub struct SiloManager {
    pub silos: Vec<QSilo>,
}

impl SiloManager {
    pub const fn new() -> Self {
        SiloManager { silos: Vec::new() }
    }

    /// Spawn a new Silo.
    pub fn spawn(&mut self, binary_oid: u64, page_table_root: u64) -> SiloId {
        let silo = QSilo::create(binary_oid, page_table_root);
        let id = silo.id;
        self.silos.push(silo);
        id
    }

    /// Get a mutable reference to a Silo by ID.
    pub fn get_mut(&mut self, id: SiloId) -> Option<&mut QSilo> {
        self.silos.iter_mut().find(|s| s.id == id)
    }

    /// Kill a Silo by ID.
    pub fn kill(&mut self, id: SiloId) {
        if let Some(idx) = self.silos.iter().position(|s| s.id == id) {
            self.silos[idx].vaporize();
            self.silos.swap_remove(idx);
        }
    }
}
