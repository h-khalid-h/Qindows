//! # USB Device Cap Bridge (Phase 194)
//!
//! ## Architecture Guardian: The Gap
//! `usb.rs` implements USB device detection:
//! - `DeviceDescriptor { usb_version, device_class: UsbClass, vendor_id, product_id, ... }`
//! - `UsbClass` — HID, MassStorage, CDC, etc. (enum)
//! - `DeviceDescriptor::from_bytes(data)` → Option<DeviceDescriptor>
//!
//! **Missing link**: USB device access was never capability-gated.
//! Any Silo could directly access HID devices (keylogger risk) or
//! MassStorage devices (data exfiltration risk).
//!
//! This module provides `UsbDeviceCapBridge`:
//! Admin:EXEC cap required before USB device access is permitted.

extern crate alloc;

use crate::usb::{DeviceDescriptor, UsbClass};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct UsbCapBridgeStats {
    pub allowed:       u64,
    pub denied:        u64,
    pub hid_count:     u64,
    pub storage_count: u64,
}

pub struct UsbDeviceCapBridge {
    pub stats: UsbCapBridgeStats,
}

impl UsbDeviceCapBridge {
    pub fn new() -> Self {
        UsbDeviceCapBridge { stats: UsbCapBridgeStats::default() }
    }

    /// Check if a Silo may access a discovered USB device — requires Admin:EXEC cap.
    pub fn authorize_device(
        &mut self,
        silo_id: u64,
        descriptor: &DeviceDescriptor,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Track sensitive device classes
        match &descriptor.device_class {
            UsbClass::Hid         => self.stats.hid_count += 1,
            UsbClass::MassStorage => self.stats.storage_count += 1,
            _ => {}
        }

        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.denied += 1;
            crate::serial_println!(
                "[USB] Silo {} denied VID={:#06x} — no Admin:EXEC cap",
                silo_id, descriptor.vendor_id
            );
            return false;
        }
        self.stats.allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UsbCapBridge: allowed={} denied={} hid={} storage={}",
            self.stats.allowed, self.stats.denied, self.stats.hid_count, self.stats.storage_count
        );
    }
}
