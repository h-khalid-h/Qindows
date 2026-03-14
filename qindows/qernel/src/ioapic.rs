//! # Qernel I/O APIC Controller
//!
//! Programs the I/O APIC for routing hardware interrupts to
//! Local APICs. Supports redirection table entry management,
//! masking, and delivery mode configuration.

/// IOAPIC register offsets.
pub mod regs {
    pub const IOAPICID: u32     = 0x00;
    pub const IOAPICVER: u32    = 0x01;
    pub const IOAPICARB: u32    = 0x02;
    pub const IOREDTBL_BASE: u32 = 0x10; // Each entry uses 2 registers
}

/// Interrupt delivery modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// Normal interrupt delivery
    Fixed = 0b000,
    /// Lowest priority among target processors
    LowestPriority = 0b001,
    /// System management interrupt
    Smi = 0b010,
    /// Non-maskable interrupt
    Nmi = 0b100,
    /// INIT signal
    Init = 0b101,
    /// External interrupt (used for PIC compat)
    ExtInt = 0b111,
}

/// Destination mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestMode {
    /// Physical APIC ID
    Physical = 0,
    /// Logical destination (set of processors)
    Logical = 1,
}

/// Trigger mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    Edge = 0,
    Level = 1,
}

/// Pin polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    ActiveHigh = 0,
    ActiveLow = 1,
}

/// A redirection table entry (64 bits).
#[derive(Debug, Clone, Copy)]
pub struct RedirectionEntry {
    /// Interrupt vector (0-255)
    pub vector: u8,
    /// Delivery mode
    pub delivery_mode: DeliveryMode,
    /// Destination mode
    pub dest_mode: DestMode,
    /// Trigger mode
    pub trigger: TriggerMode,
    /// Pin polarity
    pub polarity: Polarity,
    /// Masked (true = disabled)
    pub masked: bool,
    /// Destination APIC ID
    pub destination: u8,
}

impl RedirectionEntry {
    /// Encode to 64-bit register value.
    pub fn encode(&self) -> u64 {
        let mut val: u64 = 0;
        val |= self.vector as u64;
        val |= (self.delivery_mode as u64) << 8;
        val |= (self.dest_mode as u64) << 11;
        val |= (self.polarity as u64) << 13;
        val |= (self.trigger as u64) << 15;
        val |= if self.masked { 1 << 16 } else { 0 };
        val |= (self.destination as u64) << 56;
        val
    }

    /// Decode from 64-bit register value.
    pub fn decode(val: u64) -> Self {
        RedirectionEntry {
            vector: (val & 0xFF) as u8,
            delivery_mode: match (val >> 8) & 0x7 {
                0 => DeliveryMode::Fixed,
                1 => DeliveryMode::LowestPriority,
                2 => DeliveryMode::Smi,
                4 => DeliveryMode::Nmi,
                5 => DeliveryMode::Init,
                7 => DeliveryMode::ExtInt,
                _ => DeliveryMode::Fixed,
            },
            dest_mode: if (val >> 11) & 1 == 0 { DestMode::Physical } else { DestMode::Logical },
            polarity: if (val >> 13) & 1 == 0 { Polarity::ActiveHigh } else { Polarity::ActiveLow },
            trigger: if (val >> 15) & 1 == 0 { TriggerMode::Edge } else { TriggerMode::Level },
            masked: (val >> 16) & 1 != 0,
            destination: ((val >> 56) & 0xFF) as u8,
        }
    }
}

/// The IOAPIC controller.
pub struct IoApic {
    /// MMIO base address
    pub base_addr: u64,
    /// IOAPIC ID
    pub id: u8,
    /// Global System Interrupt base
    pub gsi_base: u32,
    /// Number of redirection entries
    pub max_entries: u8,
    /// Cache of redirection entries
    pub entries: [Option<RedirectionEntry>; 24],
    /// IRQ override map (ISA IRQ → GSI)
    pub overrides: [(u8, u32); 16],
    /// Stats
    pub irqs_routed: u64,
}

impl IoApic {
    /// Create a new IOAPIC controller.
    ///
    /// # Safety
    /// `base_addr` must point to a valid IOAPIC MMIO region.
    pub unsafe fn new(base_addr: u64, id: u8, gsi_base: u32) -> Self {
        let mut ioapic = IoApic {
            base_addr,
            id,
            gsi_base,
            max_entries: 0,
            entries: [None; 24],
            overrides: [(0, 0); 16],
            irqs_routed: 0,
        };

        // Read version register to get max entries
        let ver = ioapic.read_reg(regs::IOAPICVER);
        ioapic.max_entries = ((ver >> 16) & 0xFF) as u8 + 1;

        // Initialize default overrides (1:1 ISA)
        for i in 0..16u8 {
            ioapic.overrides[i as usize] = (i, i as u32 + gsi_base);
        }

        ioapic
    }

    /// Read an IOAPIC register.
    unsafe fn read_reg(&self, reg: u32) -> u32 {
        let sel = self.base_addr as *mut u32;
        let data = (self.base_addr + 0x10) as *mut u32;
        core::ptr::write_volatile(sel, reg);
        core::ptr::read_volatile(data)
    }

    /// Write an IOAPIC register.
    unsafe fn write_reg(&self, reg: u32, value: u32) {
        let sel = self.base_addr as *mut u32;
        let data = (self.base_addr + 0x10) as *mut u32;
        core::ptr::write_volatile(sel, reg);
        core::ptr::write_volatile(data, value);
    }

    /// Write a redirection entry.
    pub unsafe fn write_entry(&mut self, index: u8, entry: RedirectionEntry) {
        if index >= self.max_entries { return; }

        let encoded = entry.encode();
        let reg = regs::IOREDTBL_BASE + (index as u32) * 2;

        self.write_reg(reg, encoded as u32);          // Low 32 bits
        self.write_reg(reg + 1, (encoded >> 32) as u32); // High 32 bits

        self.entries[index as usize] = Some(entry);
        self.irqs_routed += 1;
    }

    /// Read a redirection entry.
    pub unsafe fn read_entry(&self, index: u8) -> Option<RedirectionEntry> {
        if index >= self.max_entries { return None; }

        let reg = regs::IOREDTBL_BASE + (index as u32) * 2;
        let lo = self.read_reg(reg) as u64;
        let hi = self.read_reg(reg + 1) as u64;

        Some(RedirectionEntry::decode(lo | (hi << 32)))
    }

    /// Mask (disable) an IRQ.
    pub unsafe fn mask(&mut self, index: u8) {
        if let Some(mut entry) = self.read_entry(index) {
            entry.masked = true;
            self.write_entry(index, entry);
        }
    }

    /// Unmask (enable) an IRQ.
    pub unsafe fn unmask(&mut self, index: u8) {
        if let Some(mut entry) = self.read_entry(index) {
            entry.masked = false;
            self.write_entry(index, entry);
        }
    }

    /// Route an ISA IRQ to a CPU vector on a specific LAPIC.
    pub unsafe fn route_irq(&mut self, irq: u8, vector: u8, lapic_id: u8) {
        let entry = RedirectionEntry {
            vector,
            delivery_mode: DeliveryMode::Fixed,
            dest_mode: DestMode::Physical,
            trigger: TriggerMode::Edge,
            polarity: Polarity::ActiveHigh,
            masked: false,
            destination: lapic_id,
        };
        self.write_entry(irq, entry);
    }

    /// Route all standard ISA IRQs (keyboard=1, timer=0, etc).
    pub unsafe fn route_isa_defaults(&mut self, base_vector: u8, lapic_id: u8) {
        for irq in 0..16u8 {
            if irq == 2 { continue; } // Skip cascade
            self.route_irq(irq, base_vector + irq, lapic_id);
        }
    }

    /// Set an ISA interrupt source override.
    pub fn set_override(&mut self, isa_irq: u8, gsi: u32) {
        if (isa_irq as usize) < self.overrides.len() {
            self.overrides[isa_irq as usize] = (isa_irq, gsi);
        }
    }

    // ── Phase 49: Silo-Aware Routing ─────────────────────────────────────────

    /// Route an IRQ pin to a Silo's assigned CPU with ownership validation.
    ///
    /// Unlike `route_irq()`, this function notes `silo_id` in the serial log
    /// and enforces that silo_id is non-zero (kernel-only if silo_id == 0).
    ///
    /// ## Q-Manifest Law 6: Silo Sandbox
    /// IRQ routing is a privileged operation. Only the `SiloInterruptRouter`
    /// should call this on behalf of a Silo after CapToken validation.
    /// Never call this directly from a syscall handler without going through
    /// `SiloInterruptRouter::allocate_vectors()` first.
    pub unsafe fn route_to_silo(
        &mut self,
        irq_pin: u8,
        vector: u8,
        lapic_id: u8,
        silo_id: u64,
        trigger: TriggerMode,
        polarity: Polarity,
    ) -> Result<(), &'static str> {
        if irq_pin >= self.max_entries {
            return Err("IOAPIC: IRQ pin out of range");
        }

        // silo_id == 0 is the kernel identity — user Silos must be non-zero
        // (They are assigned IDs starting from 1 by the Silo manager)
        if silo_id == 0 {
            return Err("IOAPIC: Cannot route kernel IOAPIC pin to Silo 0 via Silo path");
        }

        let entry = RedirectionEntry {
            vector,
            delivery_mode: DeliveryMode::Fixed,
            dest_mode: DestMode::Physical,
            trigger,
            polarity,
            masked: false,
            destination: lapic_id,
        };
        self.write_entry(irq_pin, entry);

        crate::serial_println!(
            "[IOAPIC] IRQ pin {} → vector 0x{:02x} → LAPIC {} for Silo {}",
            irq_pin, vector, lapic_id, silo_id
        );

        Ok(())
    }

    /// Mask an IRQ pin on behalf of a specific Silo.
    ///
    /// A Silo may only mask pins that were routed to it. This is enforced
    /// upstream via `SiloInterruptRouter` — this function is the hardware
    /// write path only.
    pub unsafe fn mask_for_silo(&mut self, irq_pin: u8, silo_id: u64) {
        self.mask(irq_pin);
        crate::serial_println!("[IOAPIC] Silo {} masked IRQ pin {}", silo_id, irq_pin);
    }

    /// Unmask an IRQ pin on behalf of a specific Silo.
    pub unsafe fn unmask_for_silo(&mut self, irq_pin: u8, silo_id: u64) {
        self.unmask(irq_pin);
        crate::serial_println!("[IOAPIC] Silo {} unmasked IRQ pin {}", silo_id, irq_pin);
    }
}

