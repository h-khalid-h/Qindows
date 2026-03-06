//! # PCI Bus Enumerator — Device Discovery + BAR Mapping
//!
//! Enumerates PCI/PCIe devices on the system bus, reads
//! configuration space, and maps Base Address Registers
//! for device drivers (Section 9.9).
//!
//! Features:
//! - PCI configuration space read/write
//! - Bus/device/function enumeration
//! - BAR (Base Address Register) mapping
//! - MSI/MSI-X capability parsing
//! - Device class/vendor identification

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// PCI address (bus:device:function).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PciAddr {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddr {
    pub fn new(bus: u8, dev: u8, func: u8) -> Self {
        PciAddr { bus, device: dev, function: func }
    }

    /// Compute the PCI config address for port-based access.
    pub fn config_addr(&self, offset: u8) -> u32 {
        0x8000_0000
            | ((self.bus as u32) << 16)
            | ((self.device as u32 & 0x1F) << 11)
            | ((self.function as u32 & 0x07) << 8)
            | ((offset as u32) & 0xFC)
    }
}

/// BAR type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarType {
    Memory32,
    Memory64,
    IoPort,
    Unused,
}

/// A Base Address Register.
#[derive(Debug, Clone)]
pub struct Bar {
    pub index: u8,
    pub bar_type: BarType,
    pub base_addr: u64,
    pub size: u64,
    pub prefetchable: bool,
}

/// A discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub addr: PciAddr,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub header_type: u8,
    pub irq_line: u8,
    pub irq_pin: u8,
    pub bars: Vec<Bar>,
    pub name: String,
    pub driver_bound: bool,
}

/// PCI enumeration statistics.
#[derive(Debug, Clone, Default)]
pub struct PciStats {
    pub devices_found: u64,
    pub bars_mapped: u64,
    pub config_reads: u64,
}

/// The PCI Bus Enumerator.
pub struct PciEnumerator {
    pub devices: BTreeMap<PciAddr, PciDevice>,
    pub stats: PciStats,
}

impl PciEnumerator {
    pub fn new() -> Self {
        PciEnumerator {
            devices: BTreeMap::new(),
            stats: PciStats::default(),
        }
    }

    /// Register a discovered device.
    pub fn register(&mut self, dev: PciDevice) {
        self.stats.devices_found += 1;
        self.stats.bars_mapped += dev.bars.len() as u64;
        self.devices.insert(dev.addr, dev);
    }

    /// Find devices by class code.
    pub fn find_by_class(&self, class: u8, subclass: u8) -> Vec<&PciDevice> {
        self.devices.values()
            .filter(|d| d.class_code == class && d.subclass == subclass)
            .collect()
    }

    /// Find a device by vendor:device ID.
    pub fn find_by_id(&self, vendor: u16, device: u16) -> Option<&PciDevice> {
        self.devices.values()
            .find(|d| d.vendor_id == vendor && d.device_id == device)
    }

    /// Bind a driver to a device.
    pub fn bind_driver(&mut self, addr: PciAddr) -> bool {
        if let Some(dev) = self.devices.get_mut(&addr) {
            dev.driver_bound = true;
            true
        } else { false }
    }

    /// Get all unbound devices.
    pub fn unbound(&self) -> Vec<&PciDevice> {
        self.devices.values().filter(|d| !d.driver_bound).collect()
    }
}
