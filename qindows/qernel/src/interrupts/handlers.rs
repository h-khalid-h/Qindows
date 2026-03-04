//! # Qernel Interrupt Handlers
//!
//! Exception and hardware interrupt handlers for x86-64.
//! Handles CPU faults, IRQ dispatch, and system call entry.

use core::sync::atomic::{AtomicU64, Ordering};

/// Interrupt stack frame pushed by CPU on exception entry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    /// Instruction pointer at time of interrupt
    pub rip: u64,
    /// Code segment
    pub cs: u64,
    /// CPU flags
    pub rflags: u64,
    /// Stack pointer
    pub rsp: u64,
    /// Stack segment
    pub ss: u64,
}

/// Exception error codes.
#[derive(Debug, Clone, Copy)]
pub enum ExceptionType {
    DivideByZero = 0,
    Debug = 1,
    NonMaskable = 2,
    Breakpoint = 3,
    Overflow = 4,
    BoundRange = 5,
    InvalidOpcode = 6,
    DeviceNotAvail = 7,
    DoubleFault = 8,
    InvalidTss = 10,
    SegNotPresent = 11,
    StackSegFault = 12,
    GeneralProtection = 13,
    PageFault = 14,
    X87Float = 16,
    AlignmentCheck = 17,
    MachineCheck = 18,
    SimdFloat = 19,
    Virtualization = 20,
    ControlProtection = 21,
    HypervisorInjection = 28,
    VmmCommunication = 29,
    SecurityException = 30,
}

/// Page fault error flags.
pub mod page_fault_flags {
    /// Fault caused by a non-present page
    pub const PRESENT: u64 = 1 << 0;
    /// Fault on a write access
    pub const WRITE: u64 = 1 << 1;
    /// Fault from user mode
    pub const USER: u64 = 1 << 2;
    /// Fault caused by reserved bit violation
    pub const RESERVED: u64 = 1 << 3;
    /// Fault caused by instruction fetch
    pub const INSTRUCTION_FETCH: u64 = 1 << 4;
}

/// Interrupt statistics.
pub struct InterruptStats {
    /// Count per vector (256 vectors)
    pub counts: [AtomicU64; 256],
    /// Total exceptions handled
    pub total_exceptions: AtomicU64,
    /// Total IRQs handled
    pub total_irqs: AtomicU64,
    /// Total page faults
    pub page_faults: AtomicU64,
    /// Total general protection faults
    pub gp_faults: AtomicU64,
    /// Spurious interrupts
    pub spurious: AtomicU64,
}

impl InterruptStats {
    const fn new() -> Self {
        const ZERO: AtomicU64 = AtomicU64::new(0);
        InterruptStats {
            counts: [ZERO; 256],
            total_exceptions: ZERO,
            total_irqs: ZERO,
            page_faults: ZERO,
            gp_faults: ZERO,
            spurious: ZERO,
        }
    }
}

/// Global interrupt statistics.
static STATS: InterruptStats = InterruptStats::new();

/// Handle a CPU exception.
pub fn handle_exception(vector: u8, frame: &InterruptFrame, error_code: Option<u64>) {
    STATS.counts[vector as usize].fetch_add(1, Ordering::Relaxed);
    STATS.total_exceptions.fetch_add(1, Ordering::Relaxed);

    match vector {
        0 => handle_divide_by_zero(frame),
        3 => handle_breakpoint(frame),
        6 => handle_invalid_opcode(frame),
        8 => handle_double_fault(frame, error_code.unwrap_or(0)),
        13 => handle_general_protection(frame, error_code.unwrap_or(0)),
        14 => handle_page_fault(frame, error_code.unwrap_or(0)),
        18 => handle_machine_check(frame),
        _ => {
            crate::serial_println!(
                "EXCEPTION #{}: rip={:#x} cs={:#x} rflags={:#x} err={:?}",
                vector, frame.rip, frame.cs, frame.rflags, error_code
            );
        }
    }
}

fn handle_divide_by_zero(frame: &InterruptFrame) {
    crate::serial_println!("EXCEPTION: Divide by zero at rip={:#x}", frame.rip);
    // Kill the offending Silo if user-mode, panic if kernel
    if frame.cs & 0x3 != 0 {
        // User mode  — would signal SIGFPE equivalent
        crate::serial_println!("  User-mode fault, terminating Silo");
    } else {
        panic!("Kernel divide-by-zero at {:#x}", frame.rip);
    }
}

fn handle_breakpoint(frame: &InterruptFrame) {
    crate::serial_println!("BREAKPOINT at rip={:#x}", frame.rip);
    // Would notify debugger if attached
}

fn handle_invalid_opcode(frame: &InterruptFrame) {
    crate::serial_println!("EXCEPTION: Invalid opcode at rip={:#x}", frame.rip);
    if frame.cs & 0x3 != 0 {
        crate::serial_println!("  User-mode fault, terminating Silo");
    } else {
        panic!("Kernel invalid opcode at {:#x}", frame.rip);
    }
}

fn handle_double_fault(frame: &InterruptFrame, error_code: u64) {
    panic!(
        "DOUBLE FAULT at rip={:#x} error_code={:#x} rsp={:#x}",
        frame.rip, error_code, frame.rsp
    );
}

fn handle_general_protection(frame: &InterruptFrame, error_code: u64) {
    STATS.gp_faults.fetch_add(1, Ordering::Relaxed);

    crate::serial_println!(
        "EXCEPTION: General Protection Fault at rip={:#x} error={:#x}",
        frame.rip, error_code
    );

    if frame.cs & 0x3 != 0 {
        crate::serial_println!("  User-mode GPF, terminating Silo");
    } else {
        panic!("Kernel GPF at {:#x} error={:#x}", frame.rip, error_code);
    }
}

fn handle_page_fault(frame: &InterruptFrame, error_code: u64) {
    // CR2 contains the faulting virtual address
    let faulting_addr: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) faulting_addr, options(nostack, nomem)); }

    STATS.page_faults.fetch_add(1, Ordering::Relaxed);

    let present = error_code & page_fault_flags::PRESENT != 0;
    let write = error_code & page_fault_flags::WRITE != 0;
    let user = error_code & page_fault_flags::USER != 0;
    let fetch = error_code & page_fault_flags::INSTRUCTION_FETCH != 0;

    crate::serial_println!(
        "PAGE FAULT: addr={:#x} rip={:#x} present={} write={} user={} fetch={}",
        faulting_addr, frame.rip, present, write, user, fetch
    );

    if !present && !user {
        // Kernel-mode demand paging — would allocate a frame and map it
        // For now: panic
        panic!("Kernel page fault at {:#x} (addr {:#x})", frame.rip, faulting_addr);
    }

    if user {
        // User-mode fault — would deliver SIGSEGV equivalent or do CoW
        crate::serial_println!("  User-mode page fault, terminating Silo");
    }
}

fn handle_machine_check(_frame: &InterruptFrame) {
    panic!("MACHINE CHECK EXCEPTION — hardware error detected");
}

/// Handle a hardware IRQ (vectors 32-255).
pub fn handle_irq(vector: u8) {
    STATS.counts[vector as usize].fetch_add(1, Ordering::Relaxed);
    STATS.total_irqs.fetch_add(1, Ordering::Relaxed);

    match vector {
        32 => {
            // Timer (PIT / HPET / LAPIC timer)
            // Tick the scheduler
        }
        33 => {
            // Keyboard
            // Read scancode from port 0x60
        }
        44 => {
            // Mouse (PS/2)
        }
        0xFF => {
            // Spurious interrupt
            STATS.spurious.fetch_add(1, Ordering::Relaxed);
            return; // Don't send EOI
        }
        _ => {
            // Unhandled IRQ
        }
    }

    // Send End-of-Interrupt to LAPIC
    send_eoi();
}

/// Send End-of-Interrupt to the Local APIC.
fn send_eoi() {
    // LAPIC EOI register at offset 0xB0 from LAPIC base
    // In production: write 0 to LAPIC_BASE + 0xB0
    unsafe {
        let lapic_eoi: *mut u32 = 0xFEE000B0 as *mut u32;
        core::ptr::write_volatile(lapic_eoi, 0);
    }
}

/// Get interrupt statistics.
pub fn stats() -> &'static InterruptStats {
    &STATS
}

/// Get total interrupt count for a vector.
pub fn vector_count(vector: u8) -> u64 {
    STATS.counts[vector as usize].load(Ordering::Relaxed)
}
