//! # Qernel PCI Device Scanner
//!
//! Enumerates PCI devices via ECAM (Enhanced Configuration Access
//! Mechanism), builds a device tree, and matches drivers.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// PCI device class codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciClass {
    Unclassified,
    MassStorage,
    Network,
    Display,
    Multimedia,
    Memory,
    Bridge,
    Communication,
    SystemPeripheral,
    Input,
    Docking,
    Processor,
    SerialBus,
    Wireless,
    IntelligentIo,
    Satellite,
    Encryption,
    SignalProcessing,
    Unknown(u8),
}

impl PciClass {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x00 => PciClass::Unclassified,
            0x01 => PciClass::MassStorage,
            0x02 => PciClass::Network,
            0x03 => PciClass::Display,
            0x04 => PciClass::Multimedia,
            0x05 => PciClass::Memory,
            0x06 => PciClass::Bridge,
            0x07 => PciClass::Communication,
            0x08 => PciClass::SystemPeripheral,
            0x09 => PciClass::Input,
            0x0A => PciClass::Docking,
            0x0B => PciClass::Processor,
            0x0C => PciClass::SerialBus,
            0x0D => PciClass::Wireless,
            0x0E => PciClass::IntelligentIo,
            0x0F => PciClass::Satellite,
            0x10 => PciClass::Encryption,
            0x11 => PciClass::SignalProcessing,
            _ => PciClass::Unknown(code),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            PciClass::Unclassified => "Unclassified",
            PciClass::MassStorage => "Mass Storage",
            PciClass::Network => "Network Controller",
            PciClass::Display => "Display Controller",
            PciClass::Multimedia => "Multimedia",
            PciClass::Memory => "Memory Controller",
            PciClass::Bridge => "Bridge",
            PciClass::Communication => "Communication",
            PciClass::SystemPeripheral => "System Peripheral",
            PciClass::Input => "Input Device",
            PciClass::Docking => "Docking Station",
            PciClass::Processor => "Processor",
            PciClass::SerialBus => "Serial Bus",
            PciClass::Wireless => "Wireless",
            PciClass::IntelligentIo => "Intelligent I/O",
            PciClass::Satellite => "Satellite",
            PciClass::Encryption => "Encryption",
            PciClass::SignalProcessing => "Signal Processing",
            PciClass::Unknown(_) => "Unknown",
        }
    }
}

/// A PCI Base Address Register (BAR).
#[derive(Debug, Clone, Copy)]
pub struct PciBar {
    /// BAR index (0-5)
    pub index: u8,
    /// Base address
    pub address: u64,
    /// Size
    pub size: u64,
    /// Is memory-mapped (vs I/O port)?
    pub is_memory: bool,
    /// Is 64-bit?
    pub is_64bit: bool,
    /// Is prefetchable?
    pub prefetchable: bool,
}

/// A discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// Bus number
    pub bus: u8,
    /// Device number
    pub device: u8,
    /// Function number
    pub function: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Class code
    pub class: PciClass,
    /// Subclass
    pub subclass: u8,
    /// Programming interface
    pub prog_if: u8,
    /// Revision ID
    pub revision: u8,
    /// Header type
    pub header_type: u8,
    /// Interrupt line
    pub interrupt_line: u8,
    /// Interrupt pin
    pub interrupt_pin: u8,
    /// BARs
    pub bars: Vec<PciBar>,
    /// Subsystem vendor/device
    pub subsys_vendor: u16,
    pub subsys_device: u16,
    /// Driver bound?
    pub driver_bound: bool,
    /// Driver name
    pub driver_name: Option<String>,
}

impl PciDevice {
    /// Full BDF (Bus:Device.Function) address.
    pub fn bdf(&self) -> u32 {
        ((self.bus as u32) << 16) | ((self.device as u32) << 11) | ((self.function as u32) << 8)
    }

    /// Is this a multi-function device?
    pub fn is_multifunction(&self) -> bool {
        self.header_type & 0x80 != 0
    }

    /// Is this a bridge?
    pub fn is_bridge(&self) -> bool {
        self.header_type & 0x7F == 0x01
    }
}

/// PCI configuration space read (via I/O ports 0xCF8/0xCFC).
pub unsafe fn pci_config_read(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = 0x80000000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);

    // Write address to CONFIG_ADDRESS (0xCF8)
    core::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") address, options(nomem, nostack));

    // Read data from CONFIG_DATA (0xCFC)
    let data: u32;
    core::arch::asm!("in eax, dx", in("dx") 0xCFCu16, out("eax") data, options(nomem, nostack));

    data
}

/// The PCI Scanner.
pub struct PciScanner {
    /// Discovered devices
    pub devices: Vec<PciDevice>,
    /// Stats
    pub buses_scanned: u16,
    pub devices_found: u16,
    pub bridges_found: u16,
}

impl PciScanner {
    pub fn new() -> Self {
        PciScanner {
            devices: Vec::new(),
            buses_scanned: 0,
            devices_found: 0,
            bridges_found: 0,
        }
    }

    /// Scan all PCI buses.
    pub fn scan(&mut self) {
        for bus in 0..=255u8 {
            self.scan_bus(bus);
            self.buses_scanned += 1;
        }
    }

    /// Scan a single bus.
    fn scan_bus(&mut self, bus: u8) {
        for device in 0..32u8 {
            self.scan_device(bus, device);
        }
    }

    /// Scan a single device slot.
    fn scan_device(&mut self, bus: u8, device: u8) {
        let vendor = self.read_vendor(bus, device, 0);
        if vendor == 0xFFFF { return; } // No device

        self.scan_function(bus, device, 0);

        // Check multi-function
        let header = unsafe { pci_config_read(bus, device, 0, 0x0C) };
        if (header >> 16) & 0x80 != 0 {
            for func in 1..8u8 {
                let fv = self.read_vendor(bus, device, func);
                if fv != 0xFFFF {
                    self.scan_function(bus, device, func);
                }
            }
        }
    }

    /// Scan a single function.
    fn scan_function(&mut self, bus: u8, device: u8, func: u8) {
        let reg0 = unsafe { pci_config_read(bus, device, func, 0x00) };
        let reg2 = unsafe { pci_config_read(bus, device, func, 0x08) };
        let reg3 = unsafe { pci_config_read(bus, device, func, 0x0C) };
        let regf = unsafe { pci_config_read(bus, device, func, 0x3C) };

        let vendor_id = (reg0 & 0xFFFF) as u16;
        let device_id = ((reg0 >> 16) & 0xFFFF) as u16;
        let revision = (reg2 & 0xFF) as u8;
        let prog_if = ((reg2 >> 8) & 0xFF) as u8;
        let subclass = ((reg2 >> 16) & 0xFF) as u8;
        let class_code = ((reg2 >> 24) & 0xFF) as u8;
        let header_type = ((reg3 >> 16) & 0xFF) as u8;
        let interrupt_line = (regf & 0xFF) as u8;
        let interrupt_pin = ((regf >> 8) & 0xFF) as u8;

        // Read BARs (header type 0 only)
        let mut bars = Vec::new();
        if header_type & 0x7F == 0x00 {
            let mut bar_idx = 0u8;
            while bar_idx < 6 {
                let offset = 0x10 + bar_idx as u8 * 4;
                let raw = unsafe { pci_config_read(bus, device, func, offset) };
                if raw != 0 {
                    let is_memory = (raw & 1) == 0;
                    let is_64bit = is_memory && ((raw >> 1) & 0x03) == 0x02;
                    let prefetchable = is_memory && (raw & 0x08) != 0;

                    let address = if is_memory {
                        let lo = (raw & 0xFFFFFFF0) as u64;
                        if is_64bit && bar_idx + 1 < 6 {
                            let hi_offset = 0x10 + (bar_idx + 1) as u8 * 4;
                            let hi = unsafe { pci_config_read(bus, device, func, hi_offset) };
                            lo | ((hi as u64) << 32)
                        } else {
                            lo
                        }
                    } else {
                        (raw & 0xFFFFFFFC) as u64
                    };

                    bars.push(PciBar {
                        index: bar_idx,
                        address,
                        size: 0, // Would need BAR sizing to determine
                        is_memory,
                        is_64bit,
                        prefetchable,
                    });

                    if is_64bit { bar_idx += 1; } // Skip next BAR for 64-bit
                }
                bar_idx += 1;
            }
        }

        let dev = PciDevice {
            bus, device, function: func,
            vendor_id, device_id,
            class: PciClass::from_code(class_code),
            subclass, prog_if, revision,
            header_type,
            interrupt_line, interrupt_pin,
            bars,
            subsys_vendor: 0, subsys_device: 0,
            driver_bound: false,
            driver_name: None,
        };

        if dev.is_bridge() { self.bridges_found += 1; }
        self.devices_found += 1;
        self.devices.push(dev);
    }

    fn read_vendor(&self, bus: u8, device: u8, func: u8) -> u16 {
        (unsafe { pci_config_read(bus, device, func, 0x00) } & 0xFFFF) as u16
    }

    /// Find devices by class.
    pub fn find_by_class(&self, class: PciClass) -> Vec<&PciDevice> {
        self.devices.iter().filter(|d| d.class == class).collect()
    }

    /// Find a device by vendor:device ID.
    pub fn find_by_id(&self, vendor: u16, device: u16) -> Option<&PciDevice> {
        self.devices.iter().find(|d| d.vendor_id == vendor && d.device_id == device)
    }
}
