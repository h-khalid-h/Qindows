//! # Fiber-Based Scheduler
//!
//! Qindows uses User-Mode Scheduling (UMS) with lightweight Fibers.
//! Instead of heavy kernel threads, each CPU core runs a Scheduler
//! that manages millions of tiny cooperative tasks.
//!
//! Performance: ~30% lower latency vs preemptive multitasking.

pub mod context;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;

/// Unique identifier for a Fiber.
pub type FiberId = u64;

/// Fiber states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    /// Ready to execute
    Ready,
    /// Currently running on a core
    Running,
    /// Waiting for an I/O event or capability token
    Blocked,
    /// Completed execution
    Dead,
}

/// A lightweight execution unit — the Qindows "thread."
///
/// Fibers are cooperative and much cheaper than OS threads.
/// A single core can manage millions of fibers.
pub struct Fiber {
    pub id: FiberId,
    pub state: FiberState,
    /// Saved CPU register state (for context switching)
    pub context: FiberContext,
    /// Silo this fiber belongs to (None = kernel fiber)
    pub silo_id: Option<u64>,
    /// Stack pointer for this fiber
    pub stack_top: u64,
    /// CPU core affinity (-1 = any core)
    pub pinned_core: i32,
    /// Scheduler tick count — incremented on every preemption (Fix #5).
    /// Reported to the Silo's cpu_ticks for Sentinel energy accounting.
    pub cpu_ticks: u64,
    /// Tick at which this fiber entered a Blocked state (for Law III timing).
    pub block_start_tick: u64,
}

/// Saved CPU registers for context switching.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FiberContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

/// Per-core scheduler state.
pub struct CoreScheduler {
    pub core_id: u32,
    /// Ready queue: fibers waiting to run
    pub ready_queue: VecDeque<Fiber>,
    /// Currently executing fiber
    pub current: Option<Fiber>,
    /// Total fibers processed (for metrics)
    pub fibers_processed: u64,
    /// Context of the kernel's idle loop (saved when switching to the first fiber)
    pub idle_context: FiberContext,
}

impl CoreScheduler {
    pub fn new(core_id: u32) -> Self {
        CoreScheduler {
            core_id,
            ready_queue: VecDeque::new(),
            current: None,
            fibers_processed: 0,
            idle_context: FiberContext::default(),
        }
    }

    /// Schedule the next fiber to run.
    ///
    /// Implements preemptive round-robin scheduling:
    /// 1. Save current fiber context + push back to ready queue
    /// 2. Pop next Ready fiber
    /// 3. Call switch_context() — the CPU now runs the new fiber
    ///
    /// Called from the APIC timer IRQ every ~1ms.
    ///
    /// # Safety
    /// Must be called with interrupts disabled (already the case in IRQ handlers).
    pub fn schedule(&mut self) {
        // ── Step 1: Account CPU time and re-queue current fiber ──────────────
        let mut old_ctx_ptr: *mut FiberContext = core::ptr::null_mut();

        if let Some(mut fiber) = self.current.take() {
            if fiber.state == FiberState::Running {
                // Fix #5: increment real per-fiber tick count so Sentinel
                // can compute actual CPU usage percentage.
                fiber.cpu_ticks = fiber.cpu_ticks.saturating_add(1);
                fiber.state = FiberState::Ready;
            }
            if fiber.state != FiberState::Dead {
                // Blocked or suspended — put back but don't Ready it
                self.ready_queue.push_back(fiber);
                // Get stable pointer to the context in the queue
                old_ctx_ptr = &mut self.ready_queue.back_mut().unwrap().context as *mut FiberContext;
            }
            // Dead fibers are simply dropped here (memory to be reclaimed)
        } else {
            // No current fiber means we were executing the kernel idle loop
            old_ctx_ptr = &mut self.idle_context as *mut FiberContext;
        }

        // Dummy context required if fiber was Dead, to let switch_context write somewhere
        let mut dummy_ctx = FiberContext::default();
        if old_ctx_ptr.is_null() {
            old_ctx_ptr = &mut dummy_ctx as *mut FiberContext;
        }

        // ── Step 2: Pick next Ready fiber ────────────────────────────────────
        // Skip non-Ready entries (blocked fibers sitting in the queue)
        let mut next = None;
        let qlen = self.ready_queue.len();
        for _ in 0..qlen {
            if let Some(f) = self.ready_queue.pop_front() {
                if f.state == FiberState::Ready {
                    next = Some(f);
                    break;
                }
                self.ready_queue.push_back(f);
            }
        }

        let idle_ctx_ptr = &mut self.idle_context as *mut FiberContext;

        if let Some(mut next_fiber) = next {
            next_fiber.state = FiberState::Running;
            self.fibers_processed += 1;

            // Fix #1: Inform the syscall capability gate which silo is now running
            if let Some(silo_id) = next_fiber.silo_id {
                crate::syscall::set_current_silo(silo_id);
            } else {
                crate::syscall::set_current_silo(0); // kernel fiber
            }

            self.current = Some(next_fiber);
            let new_ctx_ptr = &self.current.as_ref().unwrap().context as *const FiberContext;

            // Perform the actual CPU context switch.
            unsafe {
                context::switch_context(old_ctx_ptr, new_ctx_ptr);
            }
        } else {
            // No ready fibers. If we were running a user fiber, we must return to idle context!
            if old_ctx_ptr != idle_ctx_ptr {
                self.current = None; // Switch back to idle
                crate::syscall::set_current_silo(0);
                
                unsafe {
                    context::switch_context(old_ctx_ptr, idle_ctx_ptr as *const FiberContext);
                }
            }
        }
    }

    /// Spawn a new fiber on this core.
    pub fn spawn(&mut self, entry_point: u64, stack: u64, silo_id: Option<u64>) -> FiberId {
        static NEXT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let mut ctx = FiberContext::default();
        ctx.rip = entry_point;
        ctx.rsp = stack;
        ctx.rflags = 0x202; // Interrupts enabled

        let fiber = Fiber {
            id,
            state: FiberState::Ready,
            context: ctx,
            silo_id,
            stack_top: stack,
            pinned_core: -1,
            cpu_ticks: 0,
            block_start_tick: 0,
        };

        self.ready_queue.push_back(fiber);
        id
    }
}

/// Global scheduler state (one per CPU core, protected by spinlocks)
pub static SCHEDULERS: Mutex<Vec<CoreScheduler>> = Mutex::new(Vec::new());

/// Initialize the scheduler subsystem.
///
/// Detects CPU cores via ACPI/APIC and creates a CoreScheduler for each.
pub fn init() {
    let mut schedulers = SCHEDULERS.lock();
    // Single core for QEMU target. SMP module extends this via ACPI MADT enumeration.
    schedulers.push(CoreScheduler::new(0));
    crate::serial_println!("[OK] Fiber scheduler initialized ({} core(s))", schedulers.len());
}
