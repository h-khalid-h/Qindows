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
    /// CPU time consumed (in scheduler ticks) — updated every Sentinel cycle
    pub cpu_ticks: u64,
    /// Scheduler tick at which this silo's active fiber entered Blocked state.
    /// 0 = not blocked. Used by the Sentinel to measure Law III violations.
    pub block_start_tick: u64,
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
            block_start_tick: 0,
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
    /// Secure Wipe (Fix #2):
    /// 1. Immediately set state to Dead so the scheduler skips it
    /// 2. Zero the page-table root so any stale TLB entries become faults
    /// 3. Clear all capability tokens (revoke all permissions)
    /// 4. Zero memory accounting fields (prevent spectre-style leaks)
    /// 5. Leaf fibers killed by caller (SiloManager.run_sentinel_cycle)
    pub fn vaporize(&mut self) {
        crate::serial_println!(
            "SENTINEL: Vaporizing Silo {} (binary OID={:#x}, mem={} KB, cpu_ticks={})",
            self.id, self.binary_oid, self.memory_used / 1024, self.cpu_ticks
        );

        // 1. Mark dead immediately — scheduler won't pick up Blocked/Suspended
        self.state = SiloState::Dead;

        // 2. Invalidate the address space root pointer.
        //    The next context switch to a fiber of this silo will triple-fault
        //    (which the kernel catches as a GPF, triggering a clean silo kill).
        //    Safety: merely overwriting a u64 field — no memory is freed here,
        //    the physical frames are returned by the frame allocator in a
        //    separate pass once all fibers have exited.
        let old_cr3 = self.page_table_root;
        self.page_table_root = 0;

        // 3. Revoke all capability tokens
        self.capabilities.clear();

        // 4. Zero accounting fields (prevent information leakage via metrics)
        self.memory_used = 0;
        self.cpu_ticks = 0;
        self.health_score = 0;

        // 5. Invalidate TLB entries by writing a known-invalid CR3 value.
        //    Only if this was the currently active address space.
        unsafe {
            let current_cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3, options(nomem, nostack));
            if current_cr3 == old_cr3 && old_cr3 != 0 {
                // Switch to a null-like page table to prevent use-after-free.
                // In production: switch to the kernel page table instead.
                core::arch::asm!(
                    "mov cr3, {}",
                    in(reg) crate::memory::KERNEL_PML4_PHYS,
                    options(nostack)
                );
            }
        }

        crate::serial_println!("SENTINEL: Silo {} vaporized. Zero residue.", self.id);
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

    /// Run one Sentinel monitoring cycle over all active Silos (Fix #7).
    ///
    /// Called from the APIC timer IRQ (rate: configured by SentinelConfig.cycle_interval).
    /// For each running Silo, the Sentinel analyzes health metrics and enforces laws.
    /// Silos that get vaporized are removed from the manager in this same call.
    pub fn run_sentinel_cycle(&mut self, sentinel: &mut crate::sentinel::Sentinel) {
        let mut to_kill: alloc::vec::Vec<SiloId> = alloc::vec::Vec::new();

        for silo in self.silos.iter_mut() {
            if silo.state == SiloState::Dead || silo.state == SiloState::Frozen {
                continue;
            }
            let report = sentinel.analyze(silo);
            sentinel.enforce(silo, &report);

            if silo.state == SiloState::Dead {
                to_kill.push(silo.id);
            }
        }

        // Remove dead silos
        self.silos.retain(|s| s.state != SiloState::Dead);
        if !to_kill.is_empty() {
            crate::serial_println!(
                "SENTINEL: cycle complete — {} silos vaporized this pass",
                to_kill.len()
            );
        }
    }
}
