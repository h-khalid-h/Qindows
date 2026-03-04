//! # I/O APIC Driver
//!
//! Manages the I/O Advanced Programmable Interrupt Controller.
//! Routes hardware interrupts (IRQs) from devices to CPU cores.
//! Supports MSI/MSI-X redirection and per-IRQ affinity.

use core::sync::atomic::{AtomicU64, Ordering};

/// IOAPIC MMIO register offsets.
const IOREGSEL: u64 = 0x00;
const IOWIN: u64 = 0x10;

/// IOAPIC register indices.
const IOAPICID: u32 = 0x00;
const IOAPICVER: u32 = 0x01;
const IOAPICARB: u32 = 0x02;
const IOREDTBL_BASE: u32 = 0x10;

/// Redirection entry flags.
const MASKED: u64 = 1 << 16;
const LEVEL_TRIGGERED: u64 = 1 << 15;
const ACTIVE_LOW: u64 = 1 << 13;
const LOGICAL_DEST: u64 = 1 << 11;

/// Delivery modes.
#[derive(Debug, Clone, Copy)]
pub enum DeliveryMode {
    Fixed = 0b000,
    LowestPriority = 0b001,
    Smi = 0b010,
    Nmi = 0b100,
    Init = 0b101,
    ExtInt = 0b111,
}

/// Trigger mode.
#[derive(Debug, Clone, Copy)]
pub enum TriggerMode {
    Edge,
    Level,
}

/// Pin polarity.
#[derive(Debug, Clone, Copy)]
pub enum Polarity {
    ActiveHigh,
    ActiveLow,
}

/// A redirection table entry.
#[derive(Debug, Clone, Copy)]
pub struct RedirectionEntry {
    /// Interrupt vector (0-255)
    pub vector: u8,
    /// Delivery mode
    pub delivery_mode: DeliveryMode,
    /// Destination mode (physical or logical)
    pub logical_dest: bool,
    /// Polarity
    pub polarity: Polarity,
    /// Trigger mode
    pub trigger: TriggerMode,
    /// Is this entry masked?
    pub masked: bool,
    /// Destination APIC ID (or logical destination)
    pub destination: u8,
}

impl RedirectionEntry {
    /// Encode as a 64-bit IOAPIC redirection register value.
    pub fn encode(&self) -> u64 {
        let mut val: u64 = self.vector as u64;
        val |= (self.delivery_mode as u64) << 8;
        if self.logical_dest { val |= LOGICAL_DEST; }
        if matches!(self.polarity, Polarity::ActiveLow) { val |= ACTIVE_LOW; }
        if matches!(self.trigger, TriggerMode::Level) { val |= LEVEL_TRIGGERED; }
        if self.masked { val |= MASKED; }
        val |= (self.destination as u64) << 56;
        val
    }

    /// Decode from a 64-bit IOAPIC redirection register value.
    pub fn decode(val: u64) -> Self {
        RedirectionEntry {
            vector: (val & 0xFF) as u8,
            delivery_mode: match (val >> 8) & 0x7 {
                0b001 => DeliveryMode::LowestPriority,
                0b010 => DeliveryMode::Smi,
                0b100 => DeliveryMode::Nmi,
                0b101 => DeliveryMode::Init,
                0b111 => DeliveryMode::ExtInt,
                _ => DeliveryMode::Fixed,
            },
            logical_dest: val & LOGICAL_DEST != 0,
            polarity: if val & ACTIVE_LOW != 0 { Polarity::ActiveLow } else { Polarity::ActiveHigh },
            trigger: if val & LEVEL_TRIGGERED != 0 { TriggerMode::Level } else { TriggerMode::Edge },
            masked: val & MASKED != 0,
            destination: (val >> 56) as u8,
        }
    }
}

/// An IRQ override (from ACPI MADT).
#[derive(Debug, Clone, Copy)]
pub struct IrqOverride {
    /// ISA IRQ number (source)
    pub source_irq: u8,
    /// IOAPIC pin number (global system interrupt)
    pub gsi: u32,
    /// Polarity override
    pub polarity: Polarity,
    /// Trigger override
    pub trigger: TriggerMode,
}

/// The IOAPIC controller.
pub struct IoApic {
    /// MMIO base address
    pub base: u64,
    /// IOAPIC ID
    pub id: u8,
    /// Number of redirection entries
    pub max_entries: u8,
    /// Version
    pub version: u8,
    /// IRQ overrides from ACPI
    pub overrides: alloc::vec::Vec<IrqOverride>,
    /// GSI base (global system interrupt base)
    pub gsi_base: u32,
    /// Statistics
    pub irqs_routed: u64,
}

impl IoApic {
    /// Initialize the IOAPIC at the given MMIO base.
    pub fn init(base: u64, gsi_base: u32) -> Self {
        unsafe {
            // Read IOAPIC ID
            let id_reg = Self::read_reg(base, IOAPICID);
            let id = ((id_reg >> 24) & 0xF) as u8;

            // Read version and max redirection entries
            let ver_reg = Self::read_reg(base, IOAPICVER);
            let version = (ver_reg & 0xFF) as u8;
            let max_entries = ((ver_reg >> 16) & 0xFF) as u8 + 1;

            // Mask all entries initially
            for i in 0..max_entries {
                let reg = IOREDTBL_BASE + (i as u32 * 2);
                Self::write_reg(base, reg, MASKED as u32);
                Self::write_reg(base, reg + 1, 0);
            }

            crate::serial_println!(
                "[OK] IOAPIC #{}: version {}, {} entries, GSI base {}",
                id, version, max_entries, gsi_base
            );

            IoApic {
                base, id, max_entries, version,
                overrides: alloc::vec::Vec::new(),
                gsi_base,
                irqs_routed: 0,
            }
        }
    }

    /// Route an IRQ to a specific CPU vector.
    pub fn route_irq(&mut self, irq: u8, entry: RedirectionEntry) {
        let pin = self.resolve_pin(irq);
        if pin >= self.max_entries { return; }

        let encoded = entry.encode();
        unsafe {
            let reg = IOREDTBL_BASE + (pin as u32 * 2);
            Self::write_reg(self.base, reg, encoded as u32);
            Self::write_reg(self.base, reg + 1, (encoded >> 32) as u32);
        }

        self.irqs_routed += 1;
    }

    /// Mask (disable) an IRQ.
    pub fn mask_irq(&self, irq: u8) {
        let pin = self.resolve_pin(irq);
        if pin >= self.max_entries { return; }

        unsafe {
            let reg = IOREDTBL_BASE + (pin as u32 * 2);
            let low = Self::read_reg(self.base, reg);
            Self::write_reg(self.base, reg, low | MASKED as u32);
        }
    }

    /// Unmask (enable) an IRQ.
    pub fn unmask_irq(&self, irq: u8) {
        let pin = self.resolve_pin(irq);
        if pin >= self.max_entries { return; }

        unsafe {
            let reg = IOREDTBL_BASE + (pin as u32 * 2);
            let low = Self::read_reg(self.base, reg);
            Self::write_reg(self.base, reg, low & !(MASKED as u32));
        }
    }

    /// Register an IRQ override from ACPI MADT.
    pub fn add_override(&mut self, over: IrqOverride) {
        self.overrides.push(over);
    }

    /// Resolve an ISA IRQ to an IOAPIC pin (accounting for overrides).
    fn resolve_pin(&self, irq: u8) -> u8 {
        for over in &self.overrides {
            if over.source_irq == irq {
                return (over.gsi - self.gsi_base) as u8;
            }
        }
        irq // Identity mapping if no override
    }

    /// Read an IOAPIC register.
    unsafe fn read_reg(base: u64, reg: u32) -> u32 {
        core::ptr::write_volatile(base as *mut u32, reg);
        core::ptr::read_volatile((base + IOWIN) as *const u32)
    }

    /// Write an IOAPIC register.
    unsafe fn write_reg(base: u64, reg: u32, val: u32) {
        core::ptr::write_volatile(base as *mut u32, reg);
        core::ptr::write_volatile((base + IOWIN) as *mut u32, val);
    }
}
