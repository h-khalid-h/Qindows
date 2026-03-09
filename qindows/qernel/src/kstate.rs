//! # Kernel State
//!
//! Global kernel state accessible from syscall handlers and interrupt contexts.
//! Uses `spin::Once` for safe one-time initialization and `spin::Mutex` for
//! interior mutability.

use spin::{Mutex, Once};
use crate::silo::SiloManager;
use crate::ipc::IpcManager;
use crate::qaudit::AuditLog;
use crate::syscall_table::SyscallTable;

/// Global kernel state — initialized once during boot, accessible everywhere.
pub struct KernelState {
    /// Silo manager — tracks all active silos
    pub silo_mgr: Mutex<SiloManager>,
    /// IPC manager — tracks all Q-Ring channels
    pub ipc_mgr: Mutex<IpcManager>,
    /// Audit log — hash-chained event log
    pub audit: Mutex<AuditLog>,
    /// Syscall dispatch table with statistics
    pub syscall_table: SyscallTable,
    /// Boot timestamp (ticks since epoch)
    pub boot_timestamp: u64,
    /// Number of active CPU cores
    pub cpu_count: u32,
}

/// The global kernel state singleton.
static KERNEL: Once<KernelState> = Once::new();

/// Initialize the global kernel state (called once during boot).
pub fn init(
    silo_mgr: SiloManager,
    ipc_mgr: IpcManager,
    audit: AuditLog,
    boot_timestamp: u64,
) {
    KERNEL.call_once(|| KernelState {
        silo_mgr: Mutex::new(silo_mgr),
        ipc_mgr: Mutex::new(ipc_mgr),
        audit: Mutex::new(audit),
        syscall_table: SyscallTable::new(),
        boot_timestamp,
        cpu_count: 1, // SMP will update this
    });
}

/// Get a reference to the global kernel state.
///
/// # Panics
/// Panics if called before `init()`.
pub fn state() -> &'static KernelState {
    KERNEL.get().expect("Kernel state not initialized")
}

/// Convenience: lock the silo manager.
pub fn silos() -> spin::MutexGuard<'static, SiloManager> {
    state().silo_mgr.lock()
}

/// Convenience: lock the IPC manager.
pub fn ipc() -> spin::MutexGuard<'static, IpcManager> {
    state().ipc_mgr.lock()
}

/// Convenience: lock the audit log.
pub fn audit() -> spin::MutexGuard<'static, AuditLog> {
    state().audit.lock()
}

/// Global monotonic tick counter — incremented by the APIC timer IRQ (~1 per ms).
///
/// Used by the Sentinel to compute silo block durations for Law III enforcement.
static GLOBAL_TICK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Increment the global tick counter (called from the APIC timer IRQ handler).
#[inline(always)]
pub fn tick() {
    GLOBAL_TICK.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

/// Read the current global tick count (approximate milliseconds since boot).
#[inline(always)]
pub fn global_tick() -> u64 {
    GLOBAL_TICK.load(core::sync::atomic::Ordering::Relaxed)
}
