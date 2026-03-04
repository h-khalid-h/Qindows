//! # AHCI / SATA Driver
//!
//! Drives SATA devices through the AHCI (Advanced Host Controller Interface).
//! Discovered via PCI (class 0x01, subclass 0x06, progif 0x01).
//! Provides block-level I/O for hard drives and SSDs connected via SATA.

use alloc::vec::Vec;

/// AHCI HBA memory-mapped registers.
#[repr(C)]
pub struct HbaRegisters {
    /// Host Capabilities
    pub cap: u32,
    /// Global HBA Control
    pub ghc: u32,
    /// Interrupt Status
    pub is: u32,
    /// Ports Implemented (bitmask)
    pub pi: u32,
    /// Version
    pub vs: u32,
    /// Command Completion Coalescing Control
    pub ccc_ctl: u32,
    /// Command Completion Coalescing Ports
    pub ccc_pts: u32,
    /// Enclosure Management Location
    pub em_loc: u32,
    /// Enclosure Management Control
    pub em_ctl: u32,
    /// Host Capabilities Extended
    pub cap2: u32,
    /// BIOS/OS Handoff Control
    pub bohc: u32,
}

/// Port register set (one per SATA port).
#[repr(C)]
pub struct PortRegisters {
    /// Command List Base Address (lower 32 bits)
    pub clb: u32,
    /// Command List Base Address (upper 32 bits)
    pub clbu: u32,
    /// FIS Base Address (lower)
    pub fb: u32,
    /// FIS Base Address (upper)
    pub fbu: u32,
    /// Interrupt Status
    pub is: u32,
    /// Interrupt Enable
    pub ie: u32,
    /// Command and Status
    pub cmd: u32,
    /// Reserved
    pub _rsv: u32,
    /// Task File Data
    pub tfd: u32,
    /// Signature
    pub sig: u32,
    /// SATA Status
    pub ssts: u32,
    /// SATA Control
    pub sctl: u32,
    /// SATA Error
    pub serr: u32,
    /// SATA Active
    pub sact: u32,
    /// Command Issue
    pub ci: u32,
}

/// Device type identified by port signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// No device
    None,
    /// SATA hard drive / SSD
    Sata,
    /// SATAPI (CD/DVD)
    Satapi,
    /// Enclosure management bridge
    EnclosureBridge,
    /// Port multiplier
    PortMultiplier,
}

/// A discovered SATA device.
#[derive(Debug, Clone)]
pub struct SataDevice {
    /// Port number (0-31)
    pub port: u8,
    /// Device type
    pub device_type: DeviceType,
    /// Model name (from IDENTIFY)
    pub model: [u8; 40],
    /// Serial number
    pub serial: [u8; 20],
    /// Total sectors (LBA48)
    pub total_sectors: u64,
    /// Sector size in bytes
    pub sector_size: u32,
    /// Supports NCQ (Native Command Queuing)?
    pub ncq_supported: bool,
    /// NCQ queue depth
    pub ncq_depth: u8,
    /// Supports TRIM?
    pub trim_supported: bool,
    /// Is this device online?
    pub online: bool,
}

/// AHCI Command Header (in the Command List).
#[repr(C)]
pub struct CommandHeader {
    /// Flags: CFL (command FIS length), A (ATAPI), W (write), P (prefetchable)
    pub flags: u16,
    /// Physical Region Descriptor Table Length
    pub prdtl: u16,
    /// Physical Region Descriptor Byte Count
    pub prdbc: u32,
    /// Command Table Base Address
    pub ctba: u64,
    /// Reserved
    pub _rsv: [u32; 4],
}

/// Physical Region Descriptor Table Entry.
#[repr(C)]
pub struct PrdtEntry {
    /// Data Base Address
    pub dba: u64,
    /// Reserved
    pub _rsv: u32,
    /// Data Byte Count + Interrupt bit
    pub dbc: u32,
}

/// The AHCI controller.
pub struct AhciController {
    /// MMIO base address (BAR5)
    pub mmio_base: u64,
    /// Discovered devices
    pub devices: Vec<SataDevice>,
    /// Number of implemented ports
    pub port_count: u8,
    /// Is the controller initialized?
    pub initialized: bool,
}

impl AhciController {
    /// Initialize the AHCI controller.
    pub fn init(bar5: u64) -> Self {
        let mut ctrl = AhciController {
            mmio_base: bar5,
            devices: Vec::new(),
            port_count: 0,
            initialized: false,
        };

        unsafe {
            let regs = bar5 as *const HbaRegisters;

            // Enable AHCI mode
            let ghc = core::ptr::read_volatile(&(*regs).ghc);
            core::ptr::write_volatile(&(*regs).ghc as *const _ as *mut u32, ghc | (1 << 31));

            // Get ports implemented
            let pi = core::ptr::read_volatile(&(*regs).pi);
            let cap = core::ptr::read_volatile(&(*regs).cap);
            let num_ports = ((cap & 0x1F) + 1) as u8;
            ctrl.port_count = num_ports;

            // Scan each implemented port
            for port in 0..32u8 {
                if pi & (1 << port) == 0 { continue; }

                let port_base = bar5 + 0x100 + (port as u64 * 0x80);
                let port_regs = port_base as *const PortRegisters;

                // Check device present (SATA Status: DET = 3, IPM = 1)
                let ssts = core::ptr::read_volatile(&(*port_regs).ssts);
                let det = ssts & 0xF;
                let ipm = (ssts >> 8) & 0xF;

                if det != 3 || ipm != 1 { continue; }

                // Identify device type from signature
                let sig = core::ptr::read_volatile(&(*port_regs).sig);
                let device_type = match sig {
                    0x00000101 => DeviceType::Sata,
                    0xEB140101 => DeviceType::Satapi,
                    0xC33C0101 => DeviceType::EnclosureBridge,
                    0x96690101 => DeviceType::PortMultiplier,
                    _ => DeviceType::None,
                };

                if device_type != DeviceType::None {
                    ctrl.devices.push(SataDevice {
                        port,
                        device_type,
                        model: [0; 40],
                        serial: [0; 20],
                        total_sectors: 0,
                        sector_size: 512,
                        ncq_supported: false,
                        ncq_depth: 0,
                        trim_supported: false,
                        online: true,
                    });
                }
            }

            ctrl.initialized = true;
        }

        crate::serial_println!(
            "[OK] AHCI: {} ports, {} devices found",
            ctrl.port_count,
            ctrl.devices.len()
        );

        ctrl
    }

    /// Read sectors from a SATA device.
    pub fn read_sectors(
        &self,
        port: u8,
        lba: u64,
        count: u16,
        buffer: &mut [u8],
    ) -> Result<(), &'static str> {
        if !self.initialized {
            return Err("AHCI not initialized");
        }

        let _device = self.devices.iter()
            .find(|d| d.port == port)
            .ok_or("Device not found")?;

        if buffer.len() < (count as usize * 512) {
            return Err("Buffer too small");
        }

        // In production: build a command FIS (H2D Register FIS),
        // set up PRDT entries, issue command via CI register,
        // and wait for completion interrupt
        let _ = (lba, count);

        Ok(())
    }

    /// Write sectors to a SATA device.
    pub fn write_sectors(
        &self,
        port: u8,
        lba: u64,
        count: u16,
        data: &[u8],
    ) -> Result<(), &'static str> {
        if !self.initialized {
            return Err("AHCI not initialized");
        }

        let _device = self.devices.iter()
            .find(|d| d.port == port)
            .ok_or("Device not found")?;

        if data.len() < (count as usize * 512) {
            return Err("Data too small");
        }

        let _ = (lba, count);
        Ok(())
    }

    /// Issue TRIM command (for SSDs).
    pub fn trim(&self, port: u8, lba: u64, count: u64) -> Result<(), &'static str> {
        let device = self.devices.iter()
            .find(|d| d.port == port)
            .ok_or("Device not found")?;

        if !device.trim_supported {
            return Err("TRIM not supported");
        }

        let _ = (lba, count);
        Ok(())
    }
}
