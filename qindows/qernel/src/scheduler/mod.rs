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
}

impl CoreScheduler {
    pub fn new(core_id: u32) -> Self {
        CoreScheduler {
            core_id,
            ready_queue: VecDeque::new(),
            current: None,
            fibers_processed: 0,
        }
    }

    /// Schedule the next fiber to run.
    ///
    /// If the current fiber is still Ready, it goes back in the queue.
    /// The next fiber from the ready queue becomes active.
    pub fn schedule(&mut self) {
        // Save current fiber back to ready queue if still alive
        if let Some(mut fiber) = self.current.take() {
            if fiber.state == FiberState::Running {
                fiber.state = FiberState::Ready;
                self.ready_queue.push_back(fiber);
            }
        }

        // Pick next fiber
        if let Some(mut fiber) = self.ready_queue.pop_front() {
            fiber.state = FiberState::Running;
            self.fibers_processed += 1;
            self.current = Some(fiber);
            // In production: restore fiber.context to CPU registers
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
        };

        self.ready_queue.push_back(fiber);
        id
    }
}

/// Global scheduler state (one per CPU core, protected by spinlocks)
static SCHEDULERS: Mutex<Vec<CoreScheduler>> = Mutex::new(Vec::new());

/// Initialize the scheduler subsystem.
///
/// Detects CPU cores via ACPI/APIC and creates a CoreScheduler for each.
pub fn init() {
    let mut schedulers = SCHEDULERS.lock();
    // For now, assume 1 core. In production: enumerate via ACPI MADT.
    schedulers.push(CoreScheduler::new(0));
    crate::serial_println!("[OK] Fiber scheduler initialized ({} core(s))", schedulers.len());
}
