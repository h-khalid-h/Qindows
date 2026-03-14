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

extern "x86-interrupt" fn general_protection(frame: InterruptStackFrame, error_code: u64) {
    crate::serial_println!("EXCEPTION: General Protection Fault (#GP)");
    crate::serial_println!(
        "  RIP={:#x}  RSP={:#x}  CS={:#x}  error={:#x}",
        frame.instruction_pointer, frame.stack_pointer,
        frame.code_segment, error_code,
    );
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
    // Always safe: increments a static AtomicU64, no heap or lock required.
    crate::kstate::tick();

    // Send EOI immediately so the APIC can fire the next timer interrupt.
    unsafe { send_eoi(); }

    // Only attempt preemptive scheduling AFTER boot is complete (Phase 15 done).
    // BOOT_COMPLETE is set by kstate::signal_boot_complete() at the end of Phase 15.
    // Before that point, SCHEDULERS may not be safe to access from an interrupt
    // because heap allocations during boot can hold internal spinlocks.
    if !crate::kstate::BOOT_COMPLETE.load(core::sync::atomic::Ordering::Acquire) {
        return;
    }

    // Only preempt if there are OTHER ready fibers to switch to (not just current).
    let mut scheds = crate::scheduler::SCHEDULERS.lock();
    if let Some(core0) = scheds.first_mut() {
        if !core0.ready_queue.is_empty() && core0.current.is_some() {
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

/// The Q-Ring system call entry point (naked ASM trampoline).
///
/// When a Q-Silo executes `int 0x80`, the CPU pushes SS, RSP, RFLAGS,
/// CS, RIP onto the kernel stack and switches to Ring 0. This naked
/// handler reads the syscall number from RAX and arguments from
/// RDI, RSI, RDX, R10, R8, then calls `dispatch_syscall()`.
///
/// The result is returned in RAX. After sending APIC EOI, `iretq`
/// returns to user space.
#[unsafe(naked)]
unsafe extern "C" fn syscall_handler() {
    core::arch::naked_asm!(
        // Save all caller-saved registers (we must preserve them for iretq)
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",

        // Set up arguments for dispatch_syscall(id, arg0..arg4):
        //   User RAX = syscall number → RDI (arg 0 for SysV ABI)
        //   User RDI = arg0           → RSI (arg 1)
        //   User RSI = arg1           → RDX (arg 2)
        //   User RDX = arg2           → RCX (arg 3)
        //   User R10 = arg3           → R8  (arg 4)
        //   User R8  = arg4           → R9  (arg 5)
        //
        // Note: at this point the pushed values are on the stack.
        // The original register values are still in registers because
        // we pushed them AFTER the CPU delivered the interrupt.
        "mov r9,  r8",     // arg5 = user R8
        "mov r8,  r10",    // arg4 = user R10
        "mov rcx, rdx",    // arg3 = user RDX
        "mov rdx, rsi",    // arg2 = user RSI
        "mov rsi, rdi",    // arg1 = user RDI
        "mov rdi, rax",    // arg0 = user RAX (syscall number)

        // Call the Rust dispatcher
        "call {dispatch}",

        // RAX now holds the return value from dispatch_syscall.
        // Save it in R11 temporarily while we send EOI.
        "mov r11, rax",

        // Send APIC End-Of-Interrupt (write 0 to 0xFEE000B0)
        "mov rdi, 0xFEE000B0",
        "mov dword ptr [rdi], 0",

        // Restore caller-saved registers, but put the return value into RAX
        // (skip the original RAX restore, use dispatch result instead)
        "mov rax, r11",    // Return value → user RAX
        "pop r11",         // Restore R11
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "add rsp, 8",      // Skip the pushed original RAX (replaced by result)

        "iretq",
        dispatch = sym crate::syscall::dispatch_syscall,
    );
}

/// Send End-Of-Interrupt signal to the APIC.
unsafe fn send_eoi() {
    // Local APIC EOI register at 0xFEE000B0
    let eoi_reg = 0xFEE000B0 as *mut u32;
    core::ptr::write_volatile(eoi_reg, 0);
}
