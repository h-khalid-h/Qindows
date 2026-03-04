//! # USB xHCI Driver (Stub)
//!
//! Extensible Host Controller Interface driver for USB 3.x.
//! Discovered via PCI (class 0x0C, subclass 0x03, prog_if 0x30).
//! Handles device enumeration, transfer rings, and endpoint management.

use alloc::vec::Vec;

/// USB device speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,       // 1.5 Mbps (USB 1.0)
    Full,      // 12 Mbps  (USB 1.1)
    High,      // 480 Mbps (USB 2.0)
    Super,     // 5 Gbps   (USB 3.0)
    SuperPlus, // 10 Gbps  (USB 3.1)
}

/// USB device class codes
pub mod device_class {
    pub const HID: u8 = 0x03;        // Human Interface Device
    pub const MASS_STORAGE: u8 = 0x08;
    pub const HUB: u8 = 0x09;
    pub const VENDOR_SPECIFIC: u8 = 0xFF;
}

/// USB device descriptor (simplified)
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// Slot ID assigned by xHCI
    pub slot_id: u8,
    /// Device speed
    pub speed: UsbSpeed,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device class
    pub class: u8,
    /// Device subclass
    pub subclass: u8,
    /// Device protocol
    pub protocol: u8,
    /// Number of configurations
    pub num_configs: u8,
    /// Port number on the root hub
    pub port: u8,
    /// Is the device configured?
    pub configured: bool,
}

/// xHCI TRB (Transfer Request Block) — the fundamental command unit.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Trb {
    pub param: u64,
    pub status: u32,
    pub control: u32,
}

/// TRB types
pub mod trb_type {
    pub const NORMAL: u32 = 1;
    pub const SETUP_STAGE: u32 = 2;
    pub const DATA_STAGE: u32 = 3;
    pub const STATUS_STAGE: u32 = 4;
    pub const LINK: u32 = 6;
    pub const ENABLE_SLOT: u32 = 9;
    pub const DISABLE_SLOT: u32 = 10;
    pub const ADDRESS_DEVICE: u32 = 11;
    pub const CONFIGURE_ENDPOINT: u32 = 12;
    pub const RESET_ENDPOINT: u32 = 14;
    pub const COMMAND_COMPLETION: u32 = 33;
    pub const PORT_STATUS_CHANGE: u32 = 34;
    pub const TRANSFER: u32 = 32;
}

/// xHCI Transfer Ring — circular buffer of TRBs for a single endpoint.
pub struct TransferRing {
    pub trbs: Vec<Trb>,
    pub enqueue: usize,
    pub cycle_bit: bool,
    pub size: usize,
}

impl TransferRing {
    pub fn new(size: usize) -> Self {
        TransferRing {
            trbs: alloc::vec![Trb::default(); size],
            enqueue: 0,
            cycle_bit: true,
            size,
        }
    }

    /// Enqueue a TRB.
    pub fn enqueue_trb(&mut self, mut trb: Trb) {
        // Set cycle bit
        if self.cycle_bit {
            trb.control |= 1; // Set cycle bit
        } else {
            trb.control &= !1;
        }

        self.trbs[self.enqueue] = trb;
        self.enqueue += 1;

        // Wrap around — insert Link TRB
        if self.enqueue >= self.size - 1 {
            let mut link = Trb::default();
            link.param = self.trbs.as_ptr() as u64;
            link.control = (trb_type::LINK << 10) | if self.cycle_bit { 1 } else { 0 };
            link.control |= 1 << 1; // Toggle Cycle bit
            self.trbs[self.enqueue] = link;

            self.enqueue = 0;
            self.cycle_bit = !self.cycle_bit;
        }
    }
}

/// xHCI controller state.
pub struct XhciController {
    /// MMIO base address
    pub mmio_base: u64,
    /// Command ring
    pub command_ring: TransferRing,
    /// Discovered devices
    pub devices: Vec<UsbDevice>,
    /// Maximum device slots
    pub max_slots: u8,
    /// Maximum ports
    pub max_ports: u8,
}

impl XhciController {
    /// Initialize the xHCI controller.
    pub fn init(bar0: u64) -> Self {
        let mut ctrl = XhciController {
            mmio_base: bar0,
            command_ring: TransferRing::new(256),
            devices: Vec::new(),
            max_slots: 0,
            max_ports: 0,
        };

        unsafe {
            // Read capability registers
            let cap_length = core::ptr::read_volatile(bar0 as *const u8);
            let hcs_params1 = core::ptr::read_volatile((bar0 + 0x04) as *const u32);

            ctrl.max_slots = (hcs_params1 & 0xFF) as u8;
            ctrl.max_ports = ((hcs_params1 >> 24) & 0xFF) as u8;

            let op_base = bar0 + cap_length as u64;

            // Reset controller
            let usbcmd = (op_base + 0x00) as *mut u32;
            let usbsts = (op_base + 0x04) as *const u32;

            // Set HCRST (Host Controller Reset)
            let cmd = core::ptr::read_volatile(usbcmd);
            core::ptr::write_volatile(usbcmd, cmd | (1 << 1));

            // Wait for reset complete (HCRST bit clears)
            while core::ptr::read_volatile(usbcmd) & (1 << 1) != 0 {
                core::hint::spin_loop();
            }

            // Wait for CNR (Controller Not Ready) to clear
            while core::ptr::read_volatile(usbsts) & (1 << 11) != 0 {
                core::hint::spin_loop();
            }

            // Set max device slots
            let config = (op_base + 0x38) as *mut u32;
            core::ptr::write_volatile(config, ctrl.max_slots as u32);

            // Set command ring pointer
            let crcr = (op_base + 0x18) as *mut u64;
            core::ptr::write_volatile(
                crcr,
                ctrl.command_ring.trbs.as_ptr() as u64 | 1, // Cycle bit = 1
            );

            // Start the controller (set Run/Stop bit)
            let cmd = core::ptr::read_volatile(usbcmd);
            core::ptr::write_volatile(usbcmd, cmd | 1);
        }

        crate::serial_println!(
            "[OK] xHCI USB controller: {} slots, {} ports",
            ctrl.max_slots, ctrl.max_ports
        );

        ctrl
    }

    /// Enable a device slot (first step of USB device setup).
    pub fn enable_slot(&mut self) {
        let trb = Trb {
            param: 0,
            status: 0,
            control: trb_type::ENABLE_SLOT << 10,
        };
        self.command_ring.enqueue_trb(trb);
        // Ring the doorbell to notify the controller
    }

    /// Get all connected devices.
    pub fn connected_devices(&self) -> &[UsbDevice] {
        &self.devices
    }

    /// Get a human-readable name for a USB device class.
    pub fn class_name(class: u8) -> &'static str {
        match class {
            0x01 => "Audio",
            0x02 => "Communications",
            0x03 => "HID (Keyboard/Mouse)",
            0x06 => "Imaging",
            0x07 => "Printer",
            0x08 => "Mass Storage",
            0x09 => "Hub",
            0x0E => "Video",
            0x0F => "Personal Healthcare",
            0xE0 => "Wireless",
            0xFF => "Vendor Specific",
            _ => "Unknown",
        }
    }
}
