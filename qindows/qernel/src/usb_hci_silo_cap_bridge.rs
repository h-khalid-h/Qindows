//! # USB HCI Silo Cap Bridge (Phase 247)
//!
//! ## Architecture Guardian: The Gap
//! `usb_hci.rs` implements `UsbHci`:
//! - `UsbDevice { speed, class: UsbClass, vendor_id, product_id, ... }`
//! - `UsbClass` — HID, MassStorage, Audio, Video, Hub, ...
//! - `UsbEndpoint` — bulk/interrupt/isochronous endpoints
//!
//! **Missing link**: Any Silo could enumerate and access USB devices
//! via UsbHci. A Silo could attach to the HID device (keyboard),
//! capturing keystrokes from other Silos.
//!
//! This module provides `UsbHciSiloCapBridge`:
//! Admin:EXEC cap required for HID and MassStorage device access.

extern crate alloc;

use crate::usb_hci::UsbClass;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct UsbHciCapStats {
    pub accesses_allowed: u64,
    pub accesses_denied:  u64,
}

pub struct UsbHciSiloCapBridge {
    pub stats: UsbHciCapStats,
}

impl UsbHciSiloCapBridge {
    pub fn new() -> Self {
        UsbHciSiloCapBridge { stats: UsbHciCapStats::default() }
    }

    /// Authorize USB device access — sensitive classes require Admin:EXEC.
    pub fn authorize_access(
        &mut self,
        silo_id: u64,
        class: &UsbClass,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let needs_cap = matches!(class, UsbClass::Hid | UsbClass::MassStorage);
        if needs_cap && !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.accesses_denied += 1;
            crate::serial_println!(
                "[USB HCI] Silo {} access to {:?} device denied — no Admin:EXEC cap", silo_id, class
            );
            return false;
        }
        self.stats.accesses_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UsbHciBridge: allowed={} denied={}", self.stats.accesses_allowed, self.stats.accesses_denied
        );
    }
}
