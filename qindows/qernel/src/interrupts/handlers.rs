//! # Qernel Interrupt Handlers
//!
//! Exception and hardware interrupt handlers for x86-64.
//! Handles CPU faults, IRQ dispatch, and system call entry.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

// ── Keyboard Ring Buffer ──────────────────────────────────────────────────────
/// 64-byte circular ring buffer for decoded ASCII keystrokes.
/// Produced by IRQ 33 (PS/2 keyboard) and consumed via key_pop() / Syscall 52.
pub const KEY_BUF_SIZE: usize = 64;
static KEY_BUF: [core::sync::atomic::AtomicU8; KEY_BUF_SIZE] = {
    const Z: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
    [Z; KEY_BUF_SIZE]
};
static KEY_HEAD: AtomicUsize = AtomicUsize::new(0);
static KEY_TAIL: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn key_push(c: u8) {
    let head = KEY_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % KEY_BUF_SIZE;
    if next != KEY_TAIL.load(Ordering::Relaxed) {
        KEY_BUF[head].store(c, Ordering::Relaxed);
        KEY_HEAD.store(next, Ordering::Release);
    }
}

/// Pop a decoded ASCII byte from the keyboard ring buffer. Returns None if empty.
/// Called from Syscall 52 (SysReadKey) in syscall/mod.rs.
pub fn key_pop() -> Option<u8> {
    let tail = KEY_TAIL.load(Ordering::Acquire);
    if tail == KEY_HEAD.load(Ordering::Relaxed) { return None; }
    let c = KEY_BUF[tail].load(Ordering::Relaxed);
    KEY_TAIL.store((tail + 1) % KEY_BUF_SIZE, Ordering::Release);
    Some(c)
}

// ── Gap 17.1 — PS/2 Modifier + Extended Scancode State ───────────────────────
/// Set when an 0xE0 prefix byte has been seen; next scancode is an extended key.
static E0_SEEN:    AtomicBool = AtomicBool::new(false);
/// Left/Right Shift keys held.
static SHIFT_HELD: AtomicBool = AtomicBool::new(false);
/// Left/Right Ctrl keys held.
static CTRL_HELD:  AtomicBool = AtomicBool::new(false);

/// PS/2 Set 1 scancode -> ASCII (US QWERTY lowercase). Break codes (0x80+) ignored.
static SCAN_TO_ASCII: [u8; 0x3B] = [
//  0x00  0x01  0x02  0x03  0x04  0x05  0x06  0x07  0x08  0x09
    0,    27,   b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8',
//  0x0A  0x0B  0x0C  0x0D  0x0E  0x0F  0x10  0x11  0x12  0x13
    b'9', b'0', b'-', b'=', 8,    9,    b'q', b'w', b'e', b'r',
//  0x14  0x15  0x16  0x17  0x18  0x19  0x1A  0x1B  0x1C  0x1D
    b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', 13,   0,
//  0x1E  0x1F  0x20  0x21  0x22  0x23  0x24  0x25  0x26  0x27
    b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';',
//  0x28  0x29  0x2A  0x2B  0x2C  0x2D  0x2E  0x2F  0x30  0x31
    b'\'',b'`', 0,    b'\\',b'z', b'x', b'c', b'v', b'b', b'n',
//  0x32  0x33  0x34  0x35  0x36  0x37  0x38  0x39  0x3A
    b'm', b',', b'.', b'/', 0,    b'*', 0,    b' ', 0,
];

/// PS/2 Set 1 scancode -> shifted ASCII (Shift held, US QWERTY).
static SCAN_TO_ASCII_SHIFT: [u8; 0x3B] = [
//  0x00  0x01  0x02  0x03  0x04  0x05  0x06  0x07  0x08  0x09
    0,    27,   b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*',
//  0x0A  0x0B  0x0C  0x0D  0x0E  0x0F  0x10  0x11  0x12  0x13
    b'(', b')', b'_', b'+', 8,    9,    b'Q', b'W', b'E', b'R',
//  0x14  0x15  0x16  0x17  0x18  0x19  0x1A  0x1B  0x1C  0x1D
    b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', 13,   0,
//  0x1E  0x1F  0x20  0x21  0x22  0x23  0x24  0x25  0x26  0x27
    b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':',
//  0x28  0x29  0x2A  0x2B  0x2C  0x2D  0x2E  0x2F  0x30  0x31
    b'"', b'~', 0,    b'|', b'Z', b'X', b'C', b'V', b'B', b'N',
//  0x32  0x33  0x34  0x35  0x36  0x37  0x38  0x39  0x3A
    b'M', b'<', b'>', b'?', 0,    b'*', 0,    b' ', 0,
];

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
            // LAPIC periodic timer — fires at 1 kHz (1ms per tick)
            // 1. Advance the global kernel tick counter (kstate::global_tick() reads this)
            crate::kstate::tick();

            // 2. Fire expired software timer callbacks (oneshots, sleep, timeout)
            crate::timer::tick();

            // 3. Drive all Phase 84-280 periodic subsystem ticks once BOOT_COMPLETE is set.
            //    This enables: energy scheduler, PMC anomaly loop, telemetry flush,
            //    kstate_ext rate-limit window reset, UNS TTL sweep, etc.
            if crate::kstate::BOOT_COMPLETE.load(core::sync::atomic::Ordering::Relaxed) {
                let tick = crate::kstate::global_tick();
                crate::boot_sequence::apic_tick_hook(tick);
            }
        }
        33 => {
            // PS/2 Keyboard — Gap 17.1: full Set-1 decode with E0-prefix + modifiers
            let scancode: u8 = unsafe {
                let mut sc: u8;
                core::arch::asm!("in al, 0x60", out("al") sc, options(nostack, nomem));
                sc
            };

            // ── 0xE0 prefix: next byte is an extended key ────────────────────
            if scancode == 0xE0 {
                E0_SEEN.store(true, Ordering::Relaxed);
            } else if E0_SEEN.swap(false, Ordering::Relaxed) {
                // Extended scancode (E0 xx)
                match scancode {
                    0x48 => { key_push(0x1B); key_push(b'['); key_push(b'A'); } // Up arrow
                    0x50 => { key_push(0x1B); key_push(b'['); key_push(b'B'); } // Down arrow
                    0x4D => { key_push(0x1B); key_push(b'['); key_push(b'C'); } // Right arrow
                    0x4B => { key_push(0x1B); key_push(b'['); key_push(b'D'); } // Left arrow
                    0x53 => { key_push(0x7F); }                                  // Delete
                    0x47 => { key_push(0x1B); key_push(b'['); key_push(b'H'); } // Home
                    0x4F => { key_push(0x1B); key_push(b'['); key_push(b'F'); } // End
                    0x49 => { key_push(0x1B); key_push(b'['); key_push(b'5'); key_push(b'~'); } // PgUp
                    0x51 => { key_push(0x1B); key_push(b'['); key_push(b'6'); key_push(b'~'); } // PgDn
                    0x1D => { CTRL_HELD.store(true,  Ordering::Relaxed); } // RCtrl make
                    0x9D => { CTRL_HELD.store(false, Ordering::Relaxed); } // RCtrl break
                    0x38 => {} // RAlt make — ignore (no alt tracking yet)
                    0xB8 => {} // RAlt break
                    _ => {}    // Unknown extended key — silently drop
                }
            } else {
                // Standard Set-1 scancode
                match scancode {
                    // ── Modifier make/break codes ────────────────────────────
                    0x2A | 0x36 => { SHIFT_HELD.store(true,  Ordering::Relaxed); } // LShift/RShift make
                    0xAA | 0xB6 => { SHIFT_HELD.store(false, Ordering::Relaxed); } // LShift/RShift break
                    0x1D        => { CTRL_HELD.store(true,   Ordering::Relaxed); } // LCtrl make
                    0x9D        => { CTRL_HELD.store(false,  Ordering::Relaxed); } // LCtrl break
                    // ── Printable / control make codes ───────────────────────
                    code if code < 0x80 => {
                        let idx = code as usize;
                        if idx < SCAN_TO_ASCII.len() {
                            let shift = SHIFT_HELD.load(Ordering::Relaxed);
                            let ctrl  = CTRL_HELD.load(Ordering::Relaxed);
                            let ascii = if shift {
                                SCAN_TO_ASCII_SHIFT[idx]
                            } else {
                                SCAN_TO_ASCII[idx]
                            };
                            if ascii != 0 {
                                if ctrl && ascii.is_ascii_alphabetic() {
                                    // Ctrl+letter → control character (e.g. Ctrl+C = 0x03)
                                    key_push(ascii.to_ascii_uppercase() - b'A' + 1);
                                } else {
                                    key_push(ascii);
                                }
                            }
                        }
                    }
                    // Break codes (0x80+) for non-modifier keys — silently ignore
                    _ => {}
                }
            }
        }
        44 => {
            // PS/2 Mouse — drain the 3-byte packet sequence from port 0x60
            // Read status byte first, then dx, dy bytes if available
            let _mb: u8 = unsafe {
                let mut b: u8;
                core::arch::asm!("in al, 0x60", out("al") b, options(nostack, nomem));
                b
            };
            // TODO(Phase 16): accumulate full 3-byte mouse packet -> motion event -> Aether WM
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
/// Writes 0 to LAPIC_BASE + 0xB0 (EOI register, IA-32 spec §10.8.5).
fn send_eoi() {
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
