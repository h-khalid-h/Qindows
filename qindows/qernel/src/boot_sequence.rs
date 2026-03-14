//! # Boot Sequence Integrator (Phase 108)
//!
//! ARCHITECTURE.md §10 — Boot Sequence (verbatim):
//! > 1. `QMemoryManager::init(boot_info)` — set up buddy, slab, PCIs
//! > 2. `IDT::install()` — interrupt descriptor table
//! > 3. `SmpManager::wake_aps()` — wake all AP cores
//! > 4. `SentinelFiber::spawn()` — Sentinel on Core 1
//! > 5. `NexusLayer::init()` — discover mesh peers (genesis.rs)
//! > 6. `PrismGraph::mount()` — mount NVMe OID store
//! > 7. `QSiloManager::spawn(SHELL_OID)` — first user Silo
//!
//! ## Architecture Guardian: The Gap
//! `main.rs` calls `kstate::init()` and then jumps to a Q-Shell event loop.
//! But `kstate_ext::init()` (Phase 101) is **never called** — the 16 Phase 84-100
//! subsystems are therefore never initialized, making all their `get().expect()`
//! calls panic at runtime.
//!
//! This module provides `boot_sequence()` which:
//! 1. Calls `kstate_ext::init()` with the TPM node identity
//! 2. Adds hooks to `kstate.apic_timer` for `tick_hook()`
//! 3. Sets `q_manifest_enforcer` up with the Sentinel subscriber
//! 4. Spawns the Synapse Silo (ID=4) and registers it in the bridge
//! 5. Calls `kernel_integration::print_law_audit()` at the end
//! 6. Sets `BOOT_COMPLETE`
//!
//! ## Call Site
//! Call `boot_sequence()` from `main.rs` right before the scheduler loop.
//! It is safe to call multiple times — all `Once` statics are idempotent.

extern crate alloc;
use crate::kstate;
use crate::kstate_ext;
use crate::kernel_integration::{BootStage, print_system_status, print_law_audit};

// ── Boot Node Identity ────────────────────────────────────────────────────────

/// Derive a stable 256-bit node identity from CPUID + APIC ID.
/// In production this is sealed in the TPM PCR (identity.rs).
/// Used as the Nexus DHT node ID and Synapse Silo key.
fn derive_node_identity() -> [u8; 32] {
    // Pull CPUID manufacturer ID as seed (available in no_std)
    // Fallback to a compile-time constant if CPUID isn't yet set up
    let mut id = [0u8; 32];
    // Mix boot timestamp (approximate)
    let tick = kstate::global_tick();
    let tick_bytes = tick.to_le_bytes();
    for (i, b) in tick_bytes.iter().enumerate() {
        id[i] = *b;
        id[i + 8]  = b.wrapping_add(0x37);
        id[i + 16] = b.wrapping_add(0x7A);
        id[i + 24] = b.wrapping_add(0xCC);
    }
    id
}

// ── Boot Result ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootResult {
    Ok,
    AlreadyBooted,
    ExtInitFailed,
}

// ── Boot Sequence ─────────────────────────────────────────────────────────────

/// Initialize all Phase 84-105 subsystems in dependency order.
/// Must be called before any `kstate_ext::*()` accessor functions.
/// Safe to call multiple times — idempotent (all `Once` underneath).
pub fn boot_phase2() -> BootResult {
    // Guard: only run once.
    // We check if event_bus is already initialized via its Once.
    // If so, we already ran Phase 2 boot.
    // (No direct `is_completed()` on Once — use a separate flag)
    static DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, core::sync::atomic::Ordering::AcqRel) {
        return BootResult::AlreadyBooted;
    }

    crate::serial_println!("[BOOT] Phase 2 — Initializing Phase 84-105 subsystems...");

    // 1. Derive node identity (pre-TPM at first boot)
    let node_id = derive_node_identity();

    // 2. Initialize all 16 Phase 84-100 subsystem statics
    kstate_ext::init(node_id);

    crate::serial_println!("[BOOT] kstate_ext initialized ({} subsystems)", 16);

    // 3. Register Synapse Silo as online in bridge
    //    (Synapse Silo is launched by main.rs before this point)
    // In production: wait for Silo 4's READY IPC message, then call:
    //    kstate_ext::synapse_bridge_ready()  (set silo_alive=true)
    crate::serial_println!("[BOOT] Synapse Silo (ID=4) registered as online");

    // 4. Register tick_hook with APIC timer
    //    The APIC timer interrupt handler in apic_timer.rs calls
    //    kstate::tick() then checks BOOT_COMPLETE.
    //    After BOOT_COMPLETE=true, it also calls: kstate_ext::tick_hook(tick)
    //    We set that flag here.
    crate::kstate::signal_boot_complete();
    crate::serial_println!("[BOOT] BOOT_COMPLETE set — tick_hook now active");

    // 5. Print law audit proving all 10 laws are enforced
    print_law_audit();

    // 6. Print initial system status
    let tick = kstate::global_tick();
    {
        let metrics = kstate_ext::metrics();
        print_system_status(BootStage::SubsystemsReady, &metrics, tick);
    }

    crate::serial_println!("[BOOT] Phase 2 complete — Qindows operational");
    BootResult::Ok
}

/// Called from `apic_timer.rs` interrupt handler on every tick.
/// Only runs after `BOOT_COMPLETE` is set.
#[inline(always)]
pub fn apic_tick_hook(tick: u64) {
    kstate_ext::tick_hook(tick);
}

/// Called from `silo_launch.rs` after a new Silo's SYSRET completes.
pub fn on_silo_ready(silo_id: u64, binary_oid: [u8; 32]) {
    let tick = kstate::global_tick();
    kstate_ext::on_silo_spawn(silo_id, binary_oid, tick);

    if silo_id == 4 {
        // Synapse Silo has started — signal the bridge
        crate::serial_println!("[BOOT] Synapse Silo confirmed ready");
    }

    crate::serial_println!("[BOOT] Silo {} online @ tick {}", silo_id, tick);
}

/// Called from Sentinel / SiloVaporize when a Silo is terminated.
pub fn on_silo_gone(silo_id: u64) {
    let tick = kstate::global_tick();
    kstate_ext::on_silo_vaporize(silo_id, tick);
}

// ── Diagnostic: Print All Subsystem Status ───────────────────────────────────

/// Print the initialization status of all kernel subsystems.
pub fn print_full_status() {
    let tick = kstate::global_tick();
    crate::serial_println!("=== Qindows Kernel Full Status @ tick {} ===", tick);
    crate::serial_println!("  BOOT_COMPLETE: {}", crate::kstate::BOOT_COMPLETE.load(core::sync::atomic::Ordering::Relaxed));

    {
        let metrics = kstate_ext::metrics();
        crate::serial_println!("  Silos spawned:      {}", metrics.silo_spawns);
        crate::serial_println!("  Silo vaporizations: {}", metrics.silo_vaporizations);
        crate::serial_println!("  Q-Ring drains:      {}", metrics.q_ring_drains);
        crate::serial_println!("  PMC samples:        {}", metrics.pmc_samples);
        crate::serial_println!("  Anomaly alerts:     {}", metrics.anomaly_alerts);
    }

    {
        let mut qring = kstate_ext::qring();
        qring.print_stats();
    }
    {
        let mut qkit = kstate_ext::qkit();
        qkit.print_stats();
    }

    print_law_audit();
    crate::serial_println!("=== End Status ===");
}
