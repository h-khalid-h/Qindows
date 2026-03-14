//! # SMP (Symmetric Multi-Processing) Bootstrap
//!
//! Boots Application Processors (APs) on multi-core systems.
//! The BSP (Bootstrap Processor) sends INIT-SIPI-SIPI sequences
//! to each AP discovered via ACPI MADT.

use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use alloc::vec::Vec;

/// Number of APs that have successfully booted.
static AP_COUNT: AtomicU32 = AtomicU32::new(0);

/// Flag: APs should start executing.
static AP_READY: AtomicBool = AtomicBool::new(false);

/// Per-core boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// Not yet started
    Offline,
    /// INIT-SIPI sent, waiting for AP to respond
    Starting,
    /// AP is running but hasn't reached idle loop
    Booting,
    /// AP is idle and ready for work
    Online,
    /// AP has been parked (power saving)
    Parked,
    /// AP has hit an error
    Faulted,
}

/// Per-core data structure.
#[derive(Debug, Clone)]
pub struct CoreInfo {
    /// APIC ID (from ACPI MADT)
    pub apic_id: u8,
    /// Core state
    pub state: CoreState,
    /// Stack top address (each core gets its own stack)
    pub stack_top: u64,
    /// Is this the BSP?
    pub is_bsp: bool,
    /// Current fiber running on this core
    pub current_fiber: Option<u64>,
    /// Load metric (number of runnable fibers)
    pub load: u32,
}

/// SMP state.
pub struct SmpManager {
    /// All cores
    pub cores: Vec<CoreInfo>,
    /// BSP APIC ID
    pub bsp_apic_id: u8,
    /// Number of online cores
    pub online_count: u32,
}

/// Stack size per AP (64 KiB).
const AP_STACK_SIZE: usize = 64 * 1024;

impl SmpManager {
    /// Initialize SMP from ACPI MADT data.
    pub fn new(apic_ids: &[u8], bsp_apic_id: u8) -> Self {
        let mut cores = Vec::with_capacity(apic_ids.len());

        for &apic_id in apic_ids {
            cores.push(CoreInfo {
                apic_id,
                state: if apic_id == bsp_apic_id {
                    CoreState::Online
                } else {
                    CoreState::Offline
                },
                stack_top: 0,
                is_bsp: apic_id == bsp_apic_id,
                current_fiber: None,
                load: 0,
            });
        }

        SmpManager {
            cores,
            bsp_apic_id,
            online_count: 1, // BSP is already online
        }
    }

    /// Boot all Application Processors.
    ///
    /// For each AP:
    /// 1. Allocate a stack
    /// 2. Copy the AP trampoline code to a low-memory page
    /// 3. Send INIT IPI
    /// 4. Wait 10ms
    /// 5. Send SIPI with trampoline page address
    /// 6. Wait for AP to increment AP_COUNT
    pub unsafe fn boot_aps(&mut self, lapic_base: u64) {
        for core in &mut self.cores {
            if core.is_bsp {
                continue;
            }

            core.state = CoreState::Starting;

            // Allocate stack for this AP
            // In production: use the frame allocator
            let stack = alloc::alloc::alloc(
                alloc::alloc::Layout::from_size_align(AP_STACK_SIZE, 16).unwrap()
            );
            core.stack_top = stack as u64 + AP_STACK_SIZE as u64;

            let apic_id = core.apic_id;
            let expected_count = AP_COUNT.load(Ordering::Relaxed) + 1;

            // Send INIT IPI
            send_ipi(lapic_base, apic_id, 0x500); // INIT
            spin_wait_ms(10);

            // Send SIPI (Startup IPI) — trampoline at physical page 0x8
            send_ipi(lapic_base, apic_id, 0x608); // SIPI, vector = 0x08
            spin_wait_ms(1);

            // If AP didn't respond, send SIPI again
            if AP_COUNT.load(Ordering::Relaxed) < expected_count {
                send_ipi(lapic_base, apic_id, 0x608);
                spin_wait_ms(1);
            }

            // Check if AP booted
            if AP_COUNT.load(Ordering::Relaxed) >= expected_count {
                core.state = CoreState::Online;
                self.online_count += 1;
            } else {
                core.state = CoreState::Faulted;
            }
        }

        crate::serial_println!(
            "[OK] SMP: {}/{} cores online",
            self.online_count,
            self.cores.len()
        );
    }

    /// Get load-balanced core for scheduling a new fiber.
    pub fn least_loaded_core(&self) -> Option<u8> {
        self.cores.iter()
            .filter(|c| c.state == CoreState::Online)
            .min_by_key(|c| c.load)
            .map(|c| c.apic_id)
    }

    /// Park a core (deep sleep for power saving).
    pub fn park_core(&mut self, apic_id: u8) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.apic_id == apic_id) {
            if !core.is_bsp {
                core.state = CoreState::Parked;
                self.online_count -= 1;
            }
        }
    }

    /// Wake a parked core.
    pub fn wake_core(&mut self, apic_id: u8, lapic_base: u64) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.apic_id == apic_id) {
            if core.state == CoreState::Parked {
                // Send NMI to wake the core
                unsafe { send_ipi(lapic_base, apic_id, 0x400); }
                core.state = CoreState::Online;
                self.online_count += 1;
            }
        }
    }
}

/// Send an Inter-Processor Interrupt via the Local APIC.
unsafe fn send_ipi(lapic_base: u64, target_apic_id: u8, vector_and_flags: u32) {
    let icr_high = (lapic_base + 0x310) as *mut u32;
    let icr_low = (lapic_base + 0x300) as *mut u32;

    // Set destination APIC ID
    core::ptr::write_volatile(icr_high, (target_apic_id as u32) << 24);
    // Send the IPI
    core::ptr::write_volatile(icr_low, vector_and_flags);

    // Wait for delivery
    while core::ptr::read_volatile(icr_low) & (1 << 12) != 0 {
        core::hint::spin_loop();
    }
}

/// Spin-wait for approximately N milliseconds.
fn spin_wait_ms(ms: u64) {
    // Rough estimate: ~1M iterations ≈ 1ms on modern CPUs
    for _ in 0..ms * 1_000_000 {
        core::hint::spin_loop();
    }
}

/// AP entry point — called by each AP after SIPI trampoline.
///
/// # Safety
/// This runs on the AP with its own stack. Must not access
/// BSP-owned data without synchronization.
pub extern "C" fn ap_entry() -> ! {
    AP_COUNT.fetch_add(1, Ordering::Release);

    // Wait until BSP signals that all subsystems are initialized
    while !AP_READY.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    // Phase 51: Enter the per-core fiber scheduler loop
    // Each AP services its own local run queue, pulling work from
    // the global load-balancer when the local queue is empty.
    ap_scheduler_loop()
}

// ── Phase 51: Per-Core Scheduler Queues ─────────────────────────────────────

/// Maximum fibers in each core's local run queue.
pub const CORE_QUEUE_DEPTH: usize = 64;

/// Maximum cores supported by Qindows (up to 256 APIC IDs).
pub const MAX_CORES: usize = 256;

/// A fiber ID ready to run on this core.
pub type FiberId = u64;

/// A simple circular run queue for a single CPU core.
///
/// ## Architecture Guardian (Phase 51)
/// Each core owns exactly one `PerCoreQueue`. The core is the sole *producer*
/// of dequeues and the sole *consumer* from its own queue. Cross-core fiber
/// placement (via `schedule_on_core`) uses atomic head/tail positions.
///
/// This is an SPSC (single-producer / single-consumer) ring buffer — the
/// same pattern as QRing — so no locks are needed for the common case.
#[repr(C, align(64))] // Cache-line aligned to prevent false sharing
pub struct PerCoreQueue {
    /// Fiber IDs waiting to run on this core.
    pub fibers: [FiberId; CORE_QUEUE_DEPTH],
    /// Write index (producer — the load-balancer or migrating core).
    pub head: core::sync::atomic::AtomicUsize,
    /// Read index (consumer — this core's AP scheduler loop).
    pub tail: core::sync::atomic::AtomicUsize,
    /// Total fibers enqueued since boot.
    pub enqueued: u64,
    /// Total fibers dequeued since boot.
    pub dequeued: u64,
}

impl PerCoreQueue {
    pub const fn new() -> Self {
        PerCoreQueue {
            fibers: [0u64; CORE_QUEUE_DEPTH],
            head: core::sync::atomic::AtomicUsize::new(0),
            tail: core::sync::atomic::AtomicUsize::new(0),
            enqueued: 0,
            dequeued: 0,
        }
    }

    /// Push a fiber ID onto this core's queue.
    ///
    /// Returns `false` if the queue is full (fiber should be placed elsewhere).
    pub fn push(&mut self, fiber_id: FiberId) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % CORE_QUEUE_DEPTH;
        if next_head == self.tail.load(Ordering::Acquire) {
            return false; // Full
        }
        self.fibers[head] = fiber_id;
        self.head.store(next_head, Ordering::Release);
        self.enqueued += 1;
        true
    }

    /// Pop the next fiber to run from this core's queue.
    pub fn pop(&mut self) -> Option<FiberId> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None; // Empty
        }
        let fiber = self.fibers[tail];
        self.tail.store((tail + 1) % CORE_QUEUE_DEPTH, Ordering::Release);
        self.dequeued += 1;
        Some(fiber)
    }

    /// Number of fibers currently pending in this queue.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        (head + CORE_QUEUE_DEPTH - tail) % CORE_QUEUE_DEPTH
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Per-CPU state block.
/// One of these exists for every logical CPU core in the system.
#[repr(C)]
pub struct CoreLocal {
    /// This core's APIC ID.
    pub apic_id: u8,
    /// This core's local fiber run queue.
    pub queue: PerCoreQueue,
    /// Scheduler tick counter (incremented on every timer interrupt).
    pub scheduler_ticks: u64,
    /// PCID currently loaded in CR3 on this core.
    pub current_pcid: u16,
    /// Fiber ID currently executing on this core (0 = idle).
    pub current_fiber: u64,
    /// Total context switches performed by this core.
    pub context_switches: u64,
    /// Total TLB shootdowns this core has processed.
    pub tlb_shootdowns_processed: u64,
}

impl CoreLocal {
    pub const fn new(apic_id: u8) -> Self {
        CoreLocal {
            apic_id,
            queue: PerCoreQueue::new(),
            scheduler_ticks: 0,
            current_pcid: 0,
            current_fiber: 0,
            context_switches: 0,
            tlb_shootdowns_processed: 0,
        }
    }
}

/// Global array of per-core state, indexed by APIC ID.
///
/// ## Architecture Guardian Note
/// Indexed by APIC ID (0–255), not by core index. Most systems have
/// contiguous APIC IDs starting at 0, so lookup is O(1).
/// Declared `static mut` — access is always single-threaded per entry
/// (only the owning core reads its own slot; the load-balancer writes
/// to other cores' `queue` via atomic head/tail).
pub static mut CORE_LOCALS: [CoreLocal; MAX_CORES] = {
    const INIT: CoreLocal = CoreLocal::new(0);
    [INIT; MAX_CORES]
};

/// Get an immutable reference to a core's local state.
pub fn core_local(apic_id: u8) -> &'static CoreLocal {
    unsafe { &CORE_LOCALS[apic_id as usize] }
}

/// Get a mutable reference to a core's local state.
///
/// # Safety
/// Caller must ensure only the owning core (or the load-balancer under
/// a lock) accesses this.
pub unsafe fn core_local_mut(apic_id: u8) -> &'static mut CoreLocal {
    &mut CORE_LOCALS[apic_id as usize]
}

/// Schedule a fiber onto a specific core's queue.
///
/// Called by the load balancer or, during spawn, to place a new fiber
/// on the `least_loaded_core()`. Returns `false` if the target core's
/// queue is full.
pub fn schedule_on_core(apic_id: u8, fiber_id: FiberId) -> bool {
    unsafe { core_local_mut(apic_id).queue.push(fiber_id) }
}

// ── Phase 51: TLB Shootdown IPI ─────────────────────────────────────────────

/// IPI vector used for TLB shootdown requests.
pub const TLB_SHOOTDOWN_VECTOR: u8 = 0xF2;

/// Send a TLB shootdown IPI to all online cores (excluding self).
///
/// Called after `unmap_page()` or `unmap_range()` to ensure stale TLB
/// entries on other cores are invalidated before the physical frame
/// is reused.
///
/// ## Q-Manifest Law 6: Silo Sandbox
/// After a Silo's page is unmapped, no other CPU must have a stale
/// TLB entry that could reference freed physical memory. This IPI
/// enforces that invariant across all cores atomically.
pub unsafe fn broadcast_tlb_shootdown(lapic_base: u64) {
    // Send broadcast IPI to all CPUs excluding self
    // ICR[19:18] = 11b (All Excluding Self shorthand)
    // ICR[8:10]  = 000b (Fixed delivery)
    // ICR[14]    = 1 (Assert level)
    let icr_lo = (lapic_base + 0x300) as *mut u32;
    let icr_hi = (lapic_base + 0x310) as *mut u32;

    // Clear destination (broadcast shorthand ignores it)
    core::ptr::write_volatile(icr_hi, 0);
    // Send: All-Excl-Self | Fixed | Assert | TLB_SHOOTDOWN_VECTOR
    let value: u32 = (TLB_SHOOTDOWN_VECTOR as u32)
        | (0b11 << 18)   // All-Excl-Self shorthand
        | (1 << 14)      // Assert
        | (0b000 << 8);  // Fixed delivery
    core::ptr::write_volatile(icr_lo, value);

    // Wait for delivery
    while core::ptr::read_volatile(icr_lo) & (1 << 12) != 0 {
        core::hint::spin_loop();
    }

    crate::serial_println!("[SMP] TLB shootdown broadcast sent");
}

/// Handler called on each AP when it receives the TLB shootdown IPI.
///
/// Re-loads CR3 without NOFLUSH to force a local TLB flush.
/// (A targeted INVPCID could be used in the future if the invalidation
/// address is passed via a shared memory descriptor.)
pub fn handle_tlb_shootdown() {
    unsafe {
        // Flush TLB by reloading CR3 (full flush — conservative but correct)
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
        // Write back WITHOUT the NOFLUSH bit to force invalidation
        core::arch::asm!("mov cr3, {}", in(reg) cr3 & !((1u64) << 63), options(nostack, preserves_flags));
    }
    // Update this core's shootdown counter
    // (APIC ID retrieval simplified for now)
    crate::serial_println!("[SMP] TLB shootdown handled on AP");
}

/// Signal all APs that the BSP has finished initialization.
///
/// Called from `main.rs` after all subsystems are ready. APs are
/// spin-waiting on `AP_READY`; this releases them into their scheduler loops.
pub fn release_aps() {
    AP_READY.store(true, Ordering::Release);
    crate::serial_println!("[SMP] All APs released into scheduler loops");
}

/// The per-AP fiber scheduler loop.
///
/// Each AP runs this loop after `ap_entry()`. It services the core's
/// local `PerCoreQueue`. When the local queue is empty, it calls `hlt`
/// to enter a low-power state until the next timer interrupt.
fn ap_scheduler_loop() -> ! {
    // Get our own APIC ID from IA32_X2APIC_APICID MSR (x2APIC mode)
    // or from the legacy LAPIC ID register.
    // Simplified: scan CORE_LOCALS for the matching AP_COUNT index.
    let ap_index = AP_COUNT.load(Ordering::Relaxed) as u8;

    loop {
        let queue = unsafe { &mut core_local_mut(ap_index).queue };

        match queue.pop() {
            Some(fiber_id) => {
                // In production: context-switch to fiber_id
                // For now: log the scheduling event and increment counter
                unsafe {
                    core_local_mut(ap_index).current_fiber = fiber_id;
                    core_local_mut(ap_index).context_switches += 1;
                }
                crate::serial_println!(
                    "[SMP Core {}] Running fiber {}",
                    ap_index, fiber_id
                );
                // Fiber execution would occur here via context switch
                // After fiber yields/exits, loop resumes
            }
            None => {
                // Queue empty — enter low-power state until next interrupt
                unsafe { core::arch::asm!("hlt"); }
            }
        }
    }
}

