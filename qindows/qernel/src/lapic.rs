//! # Qernel Local APIC Driver
//!
//! Programs the Local APIC for per-CPU interrupt management,
//! inter-processor interrupts (IPIs), and the APIC timer.

/// LAPIC register offsets (from LAPIC base address).
pub mod regs {
    pub const ID: u32           = 0x020;
    pub const VERSION: u32      = 0x030;
    pub const TPR: u32          = 0x080; // Task Priority Register
    pub const APR: u32          = 0x090; // Arbitration Priority
    pub const PPR: u32          = 0x0A0; // Processor Priority
    pub const EOI: u32          = 0x0B0; // End of Interrupt
    pub const LDR: u32          = 0x0D0; // Logical Destination
    pub const DFR: u32          = 0x0E0; // Destination Format
    pub const SVR: u32          = 0x0F0; // Spurious Interrupt Vector
    pub const ISR_BASE: u32     = 0x100; // In-Service Register (8 regs)
    pub const TMR_BASE: u32     = 0x180; // Trigger Mode Register
    pub const IRR_BASE: u32     = 0x200; // Interrupt Request Register
    pub const ESR: u32          = 0x280; // Error Status Register
    pub const ICR_LO: u32       = 0x300; // Interrupt Command (low)
    pub const ICR_HI: u32       = 0x310; // Interrupt Command (high)
    pub const LVT_TIMER: u32    = 0x320;
    pub const LVT_THERMAL: u32  = 0x330;
    pub const LVT_PERF: u32     = 0x340;
    pub const LVT_LINT0: u32    = 0x350;
    pub const LVT_LINT1: u32    = 0x360;
    pub const LVT_ERROR: u32    = 0x370;
    pub const TIMER_INIT: u32   = 0x380; // Timer Initial Count
    pub const TIMER_CURR: u32   = 0x390; // Timer Current Count
    pub const TIMER_DIV: u32    = 0x3E0; // Timer Divide Configuration
}

/// APIC timer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    OneShot   = 0b00,
    Periodic  = 0b01,
    TscDeadline = 0b10,
}

/// APIC timer divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerDivider {
    Div1   = 0b1011,
    Div2   = 0b0000,
    Div4   = 0b0001,
    Div8   = 0b0010,
    Div16  = 0b0011,
    Div32  = 0b1000,
    Div64  = 0b1001,
    Div128 = 0b1010,
}

/// IPI delivery mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpiDelivery {
    Fixed         = 0b000,
    LowestPriority = 0b001,
    Smi           = 0b010,
    Nmi           = 0b100,
    Init          = 0b101,
    StartUp       = 0b110,
}

/// IPI destination shorthand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpiDest {
    /// Specific processor (use destination field)
    Specific = 0b00,
    /// Self
    ToSelf   = 0b01,
    /// All including self
    AllInclSelf = 0b10,
    /// All excluding self
    AllExclSelf = 0b11,
}

/// The Local APIC driver.
pub struct LocalApic {
    /// MMIO base address
    pub base_addr: u64,
    /// APIC ID
    pub id: u8,
    /// Version
    pub version: u8,
    /// Max LVT entries
    pub max_lvt: u8,
    /// Timer ticks per ms (calibrated)
    pub ticks_per_ms: u32,
    /// Is enabled?
    pub enabled: bool,
    /// EOIs sent
    pub eoi_count: u64,
    /// IPIs sent
    pub ipi_count: u64,
    /// Timer interrupts
    pub timer_ticks: u64,
}

impl LocalApic {
    /// Initialize the Local APIC.
    ///
    /// # Safety
    /// `base_addr` must be the valid LAPIC MMIO base.
    pub unsafe fn new(base_addr: u64) -> Self {
        let mut lapic = LocalApic {
            base_addr,
            id: 0, version: 0, max_lvt: 0,
            ticks_per_ms: 0, enabled: false,
            eoi_count: 0, ipi_count: 0, timer_ticks: 0,
        };

        // Read ID and version
        lapic.id = ((lapic.read(regs::ID) >> 24) & 0xFF) as u8;
        let ver = lapic.read(regs::VERSION);
        lapic.version = (ver & 0xFF) as u8;
        lapic.max_lvt = ((ver >> 16) & 0xFF) as u8 + 1;

        lapic
    }

    /// Enable the APIC with a spurious interrupt vector.
    pub unsafe fn enable(&mut self, spurious_vector: u8) {
        // Set spurious vector register: bit 8 = enable APIC
        let svr = (spurious_vector as u32) | (1 << 8);
        self.write(regs::SVR, svr);

        // Set task priority to 0 (accept all interrupts)
        self.write(regs::TPR, 0);

        self.enabled = true;
    }

    /// Send End of Interrupt.
    pub unsafe fn eoi(&mut self) {
        self.write(regs::EOI, 0);
        self.eoi_count += 1;
    }

    /// Configure the APIC timer.
    pub unsafe fn setup_timer(&mut self, vector: u8, mode: TimerMode, divider: TimerDivider, initial_count: u32) {
        // Set divider
        self.write(regs::TIMER_DIV, divider as u32);

        // Set LVT timer entry
        let lvt = (vector as u32) | ((mode as u32) << 17);
        self.write(regs::LVT_TIMER, lvt);

        // Set initial count (starts the timer)
        self.write(regs::TIMER_INIT, initial_count);
    }

    /// Stop the timer.
    pub unsafe fn stop_timer(&mut self) {
        self.write(regs::TIMER_INIT, 0);
        // Mask the timer LVT
        let lvt = self.read(regs::LVT_TIMER);
        self.write(regs::LVT_TIMER, lvt | (1 << 16));
    }

    /// Get current timer count.
    pub unsafe fn timer_current(&self) -> u32 {
        self.read(regs::TIMER_CURR)
    }

    /// Calibrate timer (determine ticks per ms using PIT).
    pub unsafe fn calibrate_timer(&mut self) {
        // Use PIT channel 2 for ~10ms delay
        let pit_freq: u32 = 1_193_182;
        let pit_count: u16 = (pit_freq / 100) as u16; // ~10ms

        // Program PIT channel 2
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") 0x01u8, options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8, options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (pit_count & 0xFF) as u8, options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (pit_count >> 8) as u8, options(nomem, nostack));

        // Start APIC timer counting down from max
        self.write(regs::TIMER_DIV, TimerDivider::Div16 as u32);
        self.write(regs::TIMER_INIT, 0xFFFFFFFF);

        // Wait for PIT to complete
        loop {
            let status: u8;
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") status, options(nomem, nostack));
            if status & 0x20 != 0 { break; }
        }

        // Read elapsed ticks
        let elapsed = 0xFFFFFFFF - self.read(regs::TIMER_CURR);
        self.write(regs::TIMER_INIT, 0); // Stop

        // elapsed ticks in ~10ms → ticks_per_ms
        self.ticks_per_ms = elapsed / 10;
    }

    /// Send an IPI (Inter-Processor Interrupt).
    pub unsafe fn send_ipi(&mut self, dest_apic_id: u8, vector: u8, delivery: IpiDelivery, shorthand: IpiDest) {
        // Write destination APIC ID to ICR high
        self.write(regs::ICR_HI, (dest_apic_id as u32) << 24);

        // Write vector + delivery mode + shorthand to ICR low
        let icr_lo = (vector as u32)
            | ((delivery as u32) << 8)
            | ((shorthand as u32) << 18)
            | (1 << 14); // Level = assert

        self.write(regs::ICR_LO, icr_lo);
        self.ipi_count += 1;

        // Wait for delivery
        while self.read(regs::ICR_LO) & (1 << 12) != 0 {
            core::hint::spin_loop();
        }
    }

    /// Broadcast INIT IPI to all other processors.
    pub unsafe fn send_init_all(&mut self) {
        self.send_ipi(0, 0, IpiDelivery::Init, IpiDest::AllExclSelf);
    }

    /// Send SIPI (Startup IPI) to all other processors.
    pub unsafe fn send_sipi_all(&mut self, vector_page: u8) {
        self.send_ipi(0, vector_page, IpiDelivery::StartUp, IpiDest::AllExclSelf);
    }

    /// Read a LAPIC register.
    unsafe fn read(&self, reg: u32) -> u32 {
        core::ptr::read_volatile((self.base_addr + reg as u64) as *const u32)
    }

    /// Write a LAPIC register.
    unsafe fn write(&self, reg: u32, value: u32) {
        core::ptr::write_volatile((self.base_addr + reg as u64) as *mut u32, value);
    }

    /// Read the error status register.
    pub unsafe fn read_error(&self) -> u32 {
        self.write(regs::ESR, 0); // Write before read clears it
        self.read(regs::ESR)
    }
}
