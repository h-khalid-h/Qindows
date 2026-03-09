//! # Local APIC & IO APIC Driver
//!
//! The Advanced Programmable Interrupt Controller replaces the legacy 8259 PIC.
//! Supports multi-core interrupt routing and the APIC timer for preemptive scheduling.

/// Local APIC MMIO base address (standard x86_64 location).
const LAPIC_BASE: u64 = 0xFEE0_0000;

/// Local APIC register offsets
mod regs {
    pub const ID: u32 = 0x020;
    pub const VERSION: u32 = 0x030;
    pub const TPR: u32 = 0x080;      // Task Priority Register
    pub const EOI: u32 = 0x0B0;      // End of Interrupt
    pub const SVR: u32 = 0x0F0;      // Spurious Vector Register
    pub const ESR: u32 = 0x280;      // Error Status Register
    pub const ICR_LOW: u32 = 0x300;  // Interrupt Command Register (low)
    pub const ICR_HIGH: u32 = 0x310; // Interrupt Command Register (high)
    pub const TIMER_LVT: u32 = 0x320;     // Timer Local Vector Table
    pub const TIMER_INITIAL: u32 = 0x380;  // Timer Initial Count
    pub const TIMER_CURRENT: u32 = 0x390;  // Timer Current Count
    pub const TIMER_DIVIDE: u32 = 0x3E0;   // Timer Divide Configuration
}

/// APIC timer modes
pub enum TimerMode {
    /// Fire once after N ticks
    OneShot = 0b00,
    /// Fire repeatedly every N ticks
    Periodic = 0b01,
    /// Use TSC-deadline (most precise)
    TscDeadline = 0b10,
}

/// Read a 32-bit APIC register via MMIO.
#[inline(always)]
unsafe fn lapic_read(offset: u32) -> u32 {
    let addr = (LAPIC_BASE + offset as u64) as *const u32;
    core::ptr::read_volatile(addr)
}

/// Write a 32-bit APIC register via MMIO.
#[inline(always)]
unsafe fn lapic_write(offset: u32, value: u32) {
    let addr = (LAPIC_BASE + offset as u64) as *mut u32;
    core::ptr::write_volatile(addr, value);
}

/// Initialize the Local APIC.
///
/// Steps:
/// 1. Disable the legacy 8259 PIC (mask all IRQs)
/// 2. Enable the Local APIC via the Spurious Vector Register
/// 3. Configure the APIC timer for periodic scheduling
pub fn init() {
    unsafe {
        // ── Disable legacy 8259 PIC ──
        // Mask all IRQs on both PICs
        core::arch::asm!("out 0x21, al", in("al") 0xFFu8, options(nomem, nostack));
        core::arch::asm!("out 0xA1, al", in("al") 0xFFu8, options(nomem, nostack));

        // ── Enable Local APIC ──
        // Set SVR: enable APIC + spurious vector 0xFF
        lapic_write(regs::SVR, 0x1FF);

        // Clear error status
        lapic_write(regs::ESR, 0);
        lapic_write(regs::ESR, 0);

        // Set Task Priority to 0 (accept all interrupts)
        lapic_write(regs::TPR, 0);

        // ── Configure APIC Timer (but don't start yet) ──
        // Divide by 16
        lapic_write(regs::TIMER_DIVIDE, 0x03);

        // Periodic mode, vector 32 — but masked (bit 16 = mask)
        lapic_write(regs::TIMER_LVT, 32 | (0b01 << 17) | (1 << 16));

        // Initial count = 0 — timer won't fire until start_timer() is called
        lapic_write(regs::TIMER_INITIAL, 0);

        // Send EOI for any pending interrupts
        lapic_write(regs::EOI, 0);
    }

    crate::serial_println!("[OK] Local APIC enabled — Timer configured (deferred start)");
}

/// Start the APIC timer for preemptive scheduling.
///
/// Called after the boot sequence completes to begin periodic interrupts.
/// This arms the timer at ~10ms interval for context switching.
pub fn start_timer() {
    unsafe {
        // Unmask the timer LVT entry: periodic mode, vector 32
        lapic_write(regs::TIMER_LVT, 32 | (0b01 << 17));
        // Set initial count — determines scheduling frequency
        lapic_write(regs::TIMER_INITIAL, 1_000_000);
    }

    crate::serial_println!("[OK] APIC Timer armed — preemptive scheduling active");
}

/// Send End-of-Interrupt to the Local APIC.
///
/// Must be called at the end of every interrupt handler
/// to allow the APIC to dispatch the next interrupt.
pub fn send_eoi() {
    unsafe {
        lapic_write(regs::EOI, 0);
    }
}

/// Get the current APIC ID (identifies which CPU core we're running on).
pub fn current_cpu_id() -> u32 {
    unsafe { lapic_read(regs::ID) >> 24 }
}

/// Send an Inter-Processor Interrupt (IPI) to another core.
///
/// Used to:
/// - Wake cores from HLT for fiber migration
/// - Broadcast TLB shootdowns after page table changes
/// - Signal the Sentinel on a remote core
pub fn send_ipi(target_apic_id: u32, vector: u8) {
    unsafe {
        // Set destination
        lapic_write(regs::ICR_HIGH, target_apic_id << 24);
        // Send: fixed delivery, vector
        lapic_write(regs::ICR_LOW, vector as u32);
    }
}

/// Broadcast INIT IPI to all Application Processors (AP cores).
///
/// Used during boot to wake secondary CPU cores for SMP scheduling.
pub fn wake_ap_cores() {
    unsafe {
        // Send INIT IPI to all APs
        lapic_write(regs::ICR_HIGH, 0);
        lapic_write(regs::ICR_LOW, 0x000C_4500); // INIT, all excluding self

        // Wait 10ms (simplified — in production use ACPI timer)
        for _ in 0..1_000_000 {
            core::hint::spin_loop();
        }

        // Send STARTUP IPI with vector (AP startup code at 0x8000)
        lapic_write(regs::ICR_HIGH, 0);
        lapic_write(regs::ICR_LOW, 0x000C_4608); // STARTUP, vector 0x08 (0x8000)
    }

    crate::serial_println!("[OK] IPI broadcast — waking Application Processor cores");
}

/// ─── IO APIC ───────────────────────────────────────────────────────

/// IO APIC base address (standard).
const IOAPIC_BASE: u64 = 0xFEC0_0000;

/// IO APIC register offsets
const IOREGSEL: u32 = 0x00;
const IOWIN: u32 = 0x10;

/// Read an IO APIC register.
unsafe fn ioapic_read(reg: u32) -> u32 {
    let base = IOAPIC_BASE as *mut u32;
    core::ptr::write_volatile(base.add(0), reg);
    core::ptr::read_volatile(base.byte_add(IOWIN as usize) as *const u32)
}

/// Write an IO APIC register.
unsafe fn ioapic_write(reg: u32, value: u32) {
    let base = IOAPIC_BASE as *mut u32;
    core::ptr::write_volatile(base.add(0), reg);
    core::ptr::write_volatile(base.byte_add(IOWIN as usize) as *mut u32, value);
}

/// Route an external IRQ to a specific APIC vector.
///
/// Example: route keyboard (IRQ 1) to vector 33.
pub fn route_irq(irq: u8, vector: u8, apic_id: u8) {
    let redirection_entry = (irq as u32) * 2 + 0x10;

    unsafe {
        // Low 32 bits: vector, delivery mode, polarity, trigger
        ioapic_write(redirection_entry, vector as u32);
        // High 32 bits: destination APIC ID
        ioapic_write(redirection_entry + 1, (apic_id as u32) << 24);
    }
}

/// Initialize the IO APIC — route keyboard and other hardware IRQs.
pub fn init_ioapic() {
    // Route keyboard (IRQ 1) → vector 33
    route_irq(1, 33, 0);

    // Route COM1 serial (IRQ 4) → vector 36
    route_irq(4, 36, 0);

    // Route PS/2 Mouse (IRQ 12) → vector 44
    route_irq(12, 44, 0);

    crate::serial_println!("[OK] IO APIC initialized — Keyboard:33, Serial:36, Mouse:44");
}
