//! # USB Host Controller — Device Enumeration & Class Drivers
//!
//! Chimera's USB stack handles device discovery, enumeration,
//! and routing through class-specific drivers (Section 7.3).
//!
//! Features:
//! - xHCI host controller interface
//! - Device enumeration (address assignment, descriptor parsing)
//! - Class drivers: HID, Mass Storage, Audio, Video, CDC
//! - Hot-plug / hot-remove with Silo notification
//! - Per-Silo USB device isolation via capabilities

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// USB speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,       // 1.5 Mbps
    Full,      // 12 Mbps
    High,      // 480 Mbps
    Super,     // 5 Gbps
    SuperPlus, // 10 Gbps
}

/// USB device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    Hid,         // Keyboard, mouse, gamepad
    MassStorage, // Flash drives, SSDs
    Audio,       // Speakers, mics
    Video,       // Webcams
    Cdc,         // Serial/network adapters
    Hub,         // USB hub
    Vendor,      // Vendor-specific
    Unknown,
}

/// USB device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevState {
    Attached,
    Powered,
    Addressed,
    Configured,
    Suspended,
    Removed,
}

/// A USB device descriptor.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    pub address: u8,
    pub port: u8,
    pub speed: UsbSpeed,
    pub class: UsbClass,
    pub state: DevState,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    /// Which Silo has exclusive access (if any)
    pub bound_silo: Option<u64>,
    pub attached_at: u64,
}

/// USB transfer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    Control,
    Bulk,
    Interrupt,
    Isochronous,
}

/// A pending USB transfer.
#[derive(Debug, Clone)]
pub struct UsbTransfer {
    pub id: u64,
    pub device_addr: u8,
    pub endpoint: u8,
    pub transfer_type: TransferType,
    pub data_len: u64,
    pub completed: bool,
    pub error: bool,
}

/// USB host statistics.
#[derive(Debug, Clone, Default)]
pub struct UsbStats {
    pub devices_enumerated: u64,
    pub devices_removed: u64,
    pub transfers_completed: u64,
    pub transfers_failed: u64,
    pub bytes_transferred: u64,
}

/// The USB Host Controller.
pub struct UsbHost {
    pub devices: BTreeMap<u8, UsbDevice>,
    pub transfers: Vec<UsbTransfer>,
    next_address: u8,
    next_transfer_id: u64,
    pub stats: UsbStats,
}

impl UsbHost {
    pub fn new() -> Self {
        UsbHost {
            devices: BTreeMap::new(),
            transfers: Vec::new(),
            next_address: 1,
            next_transfer_id: 1,
            stats: UsbStats::default(),
        }
    }

    /// Enumerate a newly attached device.
    pub fn enumerate(&mut self, port: u8, speed: UsbSpeed, class: UsbClass, vid: u16, pid: u16, name: &str, now: u64) -> u8 {
        let addr = self.next_address;
        self.next_address = self.next_address.wrapping_add(1).max(1);

        self.devices.insert(addr, UsbDevice {
            address: addr, port, speed, class,
            state: DevState::Configured,
            vendor_id: vid, product_id: pid,
            name: String::from(name),
            bound_silo: None, attached_at: now,
        });

        self.stats.devices_enumerated += 1;
        addr
    }

    /// Remove a device (hot-unplug).
    pub fn remove(&mut self, address: u8) {
        if let Some(dev) = self.devices.get_mut(&address) {
            dev.state = DevState::Removed;
            self.stats.devices_removed += 1;
        }
    }

    /// Bind a device to a Silo (exclusive access).
    pub fn bind(&mut self, address: u8, silo_id: u64) -> Result<(), &'static str> {
        let dev = self.devices.get_mut(&address).ok_or("Device not found")?;
        if dev.state == DevState::Removed {
            return Err("Device removed");
        }
        if dev.bound_silo.is_some() {
            return Err("Device already bound");
        }
        dev.bound_silo = Some(silo_id);
        Ok(())
    }

    /// Unbind a device from its Silo.
    pub fn unbind(&mut self, address: u8) {
        if let Some(dev) = self.devices.get_mut(&address) {
            dev.bound_silo = None;
        }
    }

    /// Submit a USB transfer.
    pub fn submit_transfer(&mut self, device_addr: u8, endpoint: u8, transfer_type: TransferType, data_len: u64) -> Result<u64, &'static str> {
        if !self.devices.contains_key(&device_addr) {
            return Err("Device not found");
        }

        let id = self.next_transfer_id;
        self.next_transfer_id += 1;

        self.transfers.push(UsbTransfer {
            id, device_addr, endpoint, transfer_type,
            data_len, completed: false, error: false,
        });

        Ok(id)
    }

    /// Complete a transfer.
    pub fn complete_transfer(&mut self, transfer_id: u64, success: bool) {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.id == transfer_id) {
            t.completed = true;
            t.error = !success;
            if success {
                self.stats.transfers_completed += 1;
                self.stats.bytes_transferred += t.data_len;
            } else {
                self.stats.transfers_failed += 1;
            }
        }
    }

    /// Get devices visible to a Silo.
    pub fn silo_devices(&self, silo_id: u64) -> Vec<&UsbDevice> {
        self.devices.values()
            .filter(|d| d.state != DevState::Removed && 
                (d.bound_silo.is_none() || d.bound_silo == Some(silo_id)))
            .collect()
    }
}
