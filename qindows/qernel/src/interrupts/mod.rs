//! # Interrupt Handling
//!
//! IDT setup, CPU exception handlers, hardware IRQ dispatching,
//! and the Q-Ring system call interface.

pub mod handlers;

use core::fmt;

/// Interrupt Descriptor Table entry
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}

impl IdtEntry {
    pub const fn empty() -> Self {
        IdtEntry {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            _reserved: 0,
        }
    }

    /// Set the handler function for this interrupt vector.
    pub fn set_handler(&mut self, handler: u64, cs: u16) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector = cs;
        self.type_attr = 0x8E; // Present, Ring 0, 64-bit Interrupt Gate
        self.ist = 0;
    }
}

/// The full IDT (256 entries).
#[repr(C, align(16))]
pub struct Idt {
    pub entries: [IdtEntry; 256],
}

impl Idt {
    pub const fn new() -> Self {
        Idt {
            entries: [IdtEntry::empty(); 256],
        }
    }
}

/// The IDT pointer structure for LIDT instruction.
#[repr(C, packed)]
pub struct IdtPointer {
    pub limit: u16,
    pub base: u64,
}

// Static IDT
static mut IDT: Idt = Idt::new();

/// Initialize the Interrupt Descriptor Table.
///
/// Sets up handlers for:
/// - CPU exceptions (Division Error, Page Fault, Double Fault, etc.)
/// - Hardware IRQs via the APIC (Keyboard, Timer, etc.)
/// - System call vector (0x80 — the Q-Ring entry point)
pub fn init() {
    unsafe {
        // ── CPU Exceptions ──
        IDT.entries[0].set_handler(division_error as *const () as u64, 0x08);
        IDT.entries[6].set_handler(invalid_opcode as *const () as u64, 0x08);
        IDT.entries[8].set_handler(double_fault as *const () as u64, 0x08);
        IDT.entries[13].set_handler(general_protection as *const () as u64, 0x08);
        IDT.entries[14].set_handler(page_fault as *const () as u64, 0x08);

        // ── Hardware IRQs (APIC mapped to vectors 32+) ──
        IDT.entries[32].set_handler(timer_handler as *const () as u64, 0x08);
        IDT.entries[33].set_handler(keyboard_handler as *const () as u64, 0x08);
        IDT.entries[44].set_handler(mouse_handler as *const () as u64, 0x08);

        // ── Q-Ring System Call (vector 0x80) ──
        IDT.entries[0x80].set_handler(syscall_handler as *const () as u64, 0x08);
        // Make the syscall gate accessible from Ring 3 (user space)
        IDT.entries[0x80].type_attr = 0xEE; // Present, Ring 3, Interrupt Gate

        // Load the IDT
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<Idt>() - 1) as u16,
            base: &IDT as *const Idt as u64,
        };
        core::arch::asm!("lidt [{}]", in(reg) &idt_ptr, options(readonly, nostack));
    }

    crate::serial_println!("[OK] Interrupt Descriptor Table loaded (256 vectors)");
}

// ── Exception Handlers ──────────────────────────────────────────────

/// Minimal interrupt stack frame pushed by CPU on interrupt entry.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

impl fmt::Display for InterruptStackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InterruptStackFrame {{ rip: {:#x}, cs: {:#x}, flags: {:#x}, rsp: {:#x}, ss: {:#x} }}",
            self.instruction_pointer, self.code_segment, self.cpu_flags,
            self.stack_pointer, self.stack_segment)
    }
}

extern "x86-interrupt" fn division_error(_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: Division Error (#DE)");
    loop { unsafe { core::arch::asm!("hlt") }; }
}

extern "x86-interrupt" fn invalid_opcode(_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: Invalid Opcode (#UD)");
    loop { unsafe { core::arch::asm!("hlt") }; }
}

extern "x86-interrupt" fn double_fault(_frame: InterruptStackFrame, _error_code: u64) -> ! {
    crate::serial_println!("!!! DOUBLE FAULT — SYSTEM HALTED !!!");
    loop { unsafe { core::arch::asm!("hlt") }; }
}

extern "x86-interrupt" fn general_protection(_frame: InterruptStackFrame, _error_code: u64) {
    crate::serial_println!("EXCEPTION: General Protection Fault (#GP)");
    crate::serial_println!("Sentinel: Q-Manifest violation detected. Silo vaporized.");
    loop { unsafe { core::arch::asm!("hlt") }; }
}

extern "x86-interrupt" fn page_fault(_frame: InterruptStackFrame, error_code: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)); }
    crate::serial_println!(
        "EXCEPTION: Page Fault (#PF) at {:#x}, error={:#x}",
        cr2, error_code
    );
    loop { unsafe { core::arch::asm!("hlt") }; }
}

// ── Hardware IRQ Handlers ───────────────────────────────────────────

extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    // Fix #8: Increment the global monotonic tick counter so the Sentinel
    // can measure per-silo block durations for Law III enforcement.
    crate::kstate::tick();

    // Send EOI before the context switch so the APIC can fire the next timer
    // interrupt even if we're still inside the new fiber's stack.
    unsafe { send_eoi(); }

    // Invoke the fiber scheduler — this may call switch_context() and resume
    // a different fiber, so control may not return here immediately.
    let mut scheds = crate::scheduler::SCHEDULERS.lock();
    if let Some(core0) = scheds.first_mut() {
        if !core0.ready_queue.is_empty() || core0.current.is_some() {
            core0.schedule();
        }
    }
    drop(scheds);
}

extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    // Read the scancode from the PS/2 controller
    let scancode: u8;
    unsafe {
        core::arch::asm!("in al, 0x60", out("al") scancode, options(nomem, nostack));
    }
    // Forward to the keyboard driver
    crate::drivers::keyboard::handle_scancode(scancode);
    unsafe { send_eoi(); }
}

extern "x86-interrupt" fn mouse_handler(_frame: InterruptStackFrame) {
    // Forward to the mouse driver
    crate::drivers::mouse::irq_handler();
    unsafe { send_eoi(); }
}

// ── System Call Handler ─────────────────────────────────────────────

/// The Q-Ring system call entry point.
///
/// When a Q-Silo executes `int 0x80`, the CPU switches to Ring 0
/// and this handler dispatches the request based on the syscall ID.
extern "x86-interrupt" fn syscall_handler(_frame: InterruptStackFrame) {
    // In production:
    // - Read syscall ID from RAX
    // - Read arguments from RDI, RSI, RDX, R10, R8, R9
    // - Dispatch to the appropriate kernel service
    // - Check capability tokens before granting access
    unsafe { send_eoi(); }
}

/// Send End-Of-Interrupt signal to the APIC.
unsafe fn send_eoi() {
    // Local APIC EOI register at 0xFEE000B0
    let eoi_reg = 0xFEE000B0 as *mut u32;
    core::ptr::write_volatile(eoi_reg, 0);
}
