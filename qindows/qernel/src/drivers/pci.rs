//! # PCI Bus Driver
//!
//! Enumerates devices on the PCI bus via MMIO configuration space.
//! Required for discovering GPUs, NVMe drives, NICs, USB controllers,
//! and other hardware. Each device is registered with the Qernel
//! and can be claimed by a user-mode Silo driver.

use alloc::vec::Vec;

/// PCI configuration space MMIO base (from ACPI MCFG table).
/// Falls back to legacy I/O ports if MCFG is not available.
static mut PCI_MMIO_BASE: u64 = 0;

/// A discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// Bus number (0-255)
    pub bus: u8,
    /// Device number (0-31)
    pub device: u8,
    /// Function number (0-7)
    pub function: u8,
    /// Vendor ID (0xFFFF = no device)
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Class code (major)
    pub class: u8,
    /// Subclass code
    pub subclass: u8,
    /// Programming interface
    pub prog_if: u8,
    /// Revision ID
    pub revision: u8,
    /// Header type
    pub header_type: u8,
    /// Base Address Registers (BARs)
    pub bars: [u64; 6],
    /// Interrupt line
    pub interrupt_line: u8,
    /// Interrupt pin
    pub interrupt_pin: u8,
}

/// Well-known PCI device classes
pub mod class {
    pub const MASS_STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const MULTIMEDIA: u8 = 0x04;
    pub const BRIDGE: u8 = 0x06;
    pub const SERIAL_BUS: u8 = 0x0C;
}

/// Well-known subclasses
pub mod subclass {
    pub const IDE: u8 = 0x01;
    pub const SATA: u8 = 0x06;
    pub const NVME: u8 = 0x08;
    pub const ETHERNET: u8 = 0x00;
    pub const VGA: u8 = 0x00;
    pub const USB: u8 = 0x03;
}

/// Read a 32-bit value from PCI configuration space.
///
/// Uses legacy I/O ports (0xCF8/0xCFC) for configuration access.
unsafe fn pci_config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = (1 << 31) // Enable bit
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    // Write address to CONFIG_ADDRESS port
    core::arch::asm!(
        "out dx, eax",
        in("dx") 0xCF8u16,
        in("eax") address,
        options(nomem, nostack)
    );

    // Read data from CONFIG_DATA port
    let result: u32;
    core::arch::asm!(
        "in eax, dx",
        in("dx") 0xCFCu16,
        out("eax") result,
        options(nomem, nostack)
    );

    result
}

/// Read a 16-bit value from PCI configuration space.
unsafe fn pci_config_read16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let val = pci_config_read32(bus, device, function, offset & 0xFC);
    ((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

/// Read an 8-bit value from PCI configuration space.
unsafe fn pci_config_read8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let val = pci_config_read32(bus, device, function, offset & 0xFC);
    ((val >> ((offset & 3) * 8)) & 0xFF) as u8
}

/// Enumerate a single PCI function.
unsafe fn probe_function(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let vendor_id = pci_config_read16(bus, device, function, 0x00);
    if vendor_id == 0xFFFF {
        return None; // No device present
    }

    let device_id = pci_config_read16(bus, device, function, 0x02);
    let revision = pci_config_read8(bus, device, function, 0x08);
    let prog_if = pci_config_read8(bus, device, function, 0x09);
    let subclass = pci_config_read8(bus, device, function, 0x0A);
    let class = pci_config_read8(bus, device, function, 0x0B);
    let header_type = pci_config_read8(bus, device, function, 0x0E);
    let interrupt_line = pci_config_read8(bus, device, function, 0x3C);
    let interrupt_pin = pci_config_read8(bus, device, function, 0x3D);

    // Read BARs (Base Address Registers)
    let mut bars = [0u64; 6];
    for i in 0..6u8 {
        let bar_offset = 0x10 + i * 4;
        let bar_val = pci_config_read32(bus, device, function, bar_offset);
        bars[i as usize] = bar_val as u64;

        // If 64-bit BAR, read the upper 32 bits too
        if bar_val & 0x04 != 0 && i < 5 {
            let upper = pci_config_read32(bus, device, function, bar_offset + 4);
            bars[i as usize] |= (upper as u64) << 32;
        }
    }

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        revision,
        header_type: header_type & 0x7F,
        bars,
        interrupt_line,
        interrupt_pin,
    })
}

/// Scan all PCI buses and return discovered devices.
pub fn enumerate() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    unsafe {
        for bus in 0..=255u16 {
            for device in 0..32u8 {
                let vendor = pci_config_read16(bus as u8, device, 0, 0x00);
                if vendor == 0xFFFF {
                    continue;
                }

                if let Some(dev) = probe_function(bus as u8, device, 0) {
                    let is_multifunction = dev.header_type & 0x80 != 0;
                    devices.push(dev);

                    // Check additional functions for multi-function devices
                    if is_multifunction {
                        for function in 1..8u8 {
                            if let Some(dev) = probe_function(bus as u8, device, function) {
                                devices.push(dev);
                            }
                        }
                    }
                }
            }

            // Break early if we've scanned bus 255
            if bus == 255 {
                break;
            }
        }
    }

    crate::serial_println!("[OK] PCI: {} device(s) discovered", devices.len());
    devices
}

/// Find a PCI device by class and subclass.
pub fn find_by_class(devices: &[PciDevice], class: u8, subclass: u8) -> Option<&PciDevice> {
    devices.iter().find(|d| d.class == class && d.subclass == subclass)
}

/// Get a human-readable description of a PCI device class.
pub fn class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x01, 0x01) => "IDE Controller",
        (0x01, 0x06) => "SATA Controller",
        (0x01, 0x08) => "NVMe Controller",
        (0x02, 0x00) => "Ethernet Controller",
        (0x02, 0x80) => "Network Controller",
        (0x03, 0x00) => "VGA Compatible Controller",
        (0x03, 0x02) => "3D Controller",
        (0x04, 0x01) => "Audio Controller",
        (0x06, 0x00) => "Host Bridge",
        (0x06, 0x01) => "ISA Bridge",
        (0x06, 0x04) => "PCI-to-PCI Bridge",
        (0x0C, 0x03) => "USB Controller",
        _ => "Unknown Device",
    }
}
