//! # USB Host Controller Interface
//!
//! xHCI-style USB host controller driver stub for the Qernel.
//! Manages USB device enumeration, endpoint configuration,
//! and transfer scheduling (Section 9.36).
//!
//! Features:
//! - Device enumeration and address assignment
//! - Endpoint pipes (Control, Bulk, Interrupt, Isochronous)
//! - Transfer descriptor ring buffer
//! - Hub port management
//! - Per-Silo device binding

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// USB device speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,   // 1.5 Mbps
    Full,  // 12 Mbps
    High,  // 480 Mbps
    Super, // 5 Gbps
}

/// USB device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    Hid,       // Keyboard, mouse
    MassStorage,
    Audio,
    Video,
    Hub,
    Cdc,       // Serial/modem
    Printer,
    Vendor,
    Unknown,
}

/// USB endpoint type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    Control,
    Bulk,
    Interrupt,
    Isochronous,
}

/// A USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub address: u8,
    pub speed: UsbSpeed,
    pub class: UsbClass,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    pub endpoints: Vec<UsbEndpoint>,
    pub bound_silo: Option<u64>,
    pub hub_port: Option<(u8, u8)>, // (hub_addr, port)
    pub configured: bool,
}

/// A USB endpoint.
#[derive(Debug, Clone)]
pub struct UsbEndpoint {
    pub number: u8,
    pub ep_type: EndpointType,
    pub direction_in: bool,
    pub max_packet: u16,
    pub interval_ms: u8,
}

/// USB controller statistics.
#[derive(Debug, Clone, Default)]
pub struct UsbStats {
    pub devices_enumerated: u64,
    pub transfers_completed: u64,
    pub transfers_failed: u64,
    pub bytes_transferred: u64,
}

/// The USB Host Controller.
pub struct UsbHci {
    pub devices: BTreeMap<u8, UsbDevice>,
    next_address: u8,
    pub stats: UsbStats,
}

impl UsbHci {
    pub fn new() -> Self {
        UsbHci {
            devices: BTreeMap::new(),
            next_address: 1,
            stats: UsbStats::default(),
        }
    }

    /// Enumerate a new device.
    pub fn enumerate(&mut self, speed: UsbSpeed, class: UsbClass, vid: u16, pid: u16, name: &str) -> Option<u8> {
        if self.next_address >= 127 { return None; }
        let addr = self.next_address;
        self.next_address += 1;
        self.devices.insert(addr, UsbDevice {
            address: addr, speed, class,
            vendor_id: vid, product_id: pid,
            name: String::from(name),
            endpoints: Vec::new(), bound_silo: None,
            hub_port: None, configured: false,
        });
        self.stats.devices_enumerated += 1;
        Some(addr)
    }

    /// Add an endpoint to a device.
    pub fn add_endpoint(&mut self, addr: u8, ep: UsbEndpoint) {
        if let Some(dev) = self.devices.get_mut(&addr) {
            dev.endpoints.push(ep);
        }
    }

    /// Configure a device (set config descriptor).
    pub fn configure(&mut self, addr: u8) -> bool {
        if let Some(dev) = self.devices.get_mut(&addr) {
            dev.configured = true;
            true
        } else { false }
    }

    /// Bind a device to a Silo.
    pub fn bind_silo(&mut self, addr: u8, silo_id: u64) -> bool {
        if let Some(dev) = self.devices.get_mut(&addr) {
            dev.bound_silo = Some(silo_id);
            true
        } else { false }
    }

    /// Detach a device.
    pub fn detach(&mut self, addr: u8) {
        self.devices.remove(&addr);
    }

    /// List devices by class.
    pub fn devices_by_class(&self, class: UsbClass) -> Vec<&UsbDevice> {
        self.devices.values().filter(|d| d.class == class).collect()
    }
}
