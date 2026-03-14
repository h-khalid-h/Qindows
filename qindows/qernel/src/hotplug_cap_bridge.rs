//! # Hotplug Event Cap Bridge (Phase 209)
//!
//! ## Architecture Guardian: The Gap
//! `hotplug.rs` implements:
//! - `HotplugEvent { bus: HotplugBus, action: HotplugAction, device_location, ... }`
//! - `HotplugBus` — PCIe, USB, NVMe, ...
//! - `HotplugAction` — Attached, Detached, Error
//! - `HotplugPolicy` — governs how devices are assigned on plug event
//!
//! **Missing link**: Hotplug events triggered device assignment without
//! CapToken verification. A PCIe hotplug could auto-assign to any Silo.
//!
//! This module provides `HotplugCapBridge`:
//! Admin:EXEC cap required before any device is assigned on hotplug.

extern crate alloc;

use crate::hotplug::{HotplugEvent, HotplugAction, HotplugBus};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct HotplugCapStats {
    pub events_authorized: u64,
    pub events_denied:     u64,
}

pub struct HotplugCapBridge {
    pub stats: HotplugCapStats,
}

impl HotplugCapBridge {
    pub fn new() -> Self {
        HotplugCapBridge { stats: HotplugCapStats::default() }
    }

    /// Authorize processing a hotplug Attached event — requires Admin:EXEC cap.
    pub fn authorize_attach(
        &mut self,
        silo_id: u64,
        event: &HotplugEvent,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Only gate Add events — Remove/Eject/Reset are informational
        if !matches!(event.action, HotplugAction::Add) {
            return true;
        }

        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.events_denied += 1;
            crate::serial_println!(
                "[HOTPLUG] Silo {} attach {:?} denied — no Admin:EXEC cap", silo_id, event.bus
            );
            return false;
        }
        self.stats.events_authorized += 1;
        crate::serial_println!("[HOTPLUG] Silo {} authorized {:?} attach", silo_id, event.bus);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  HotplugCapBridge: authorized={} denied={}",
            self.stats.events_authorized, self.stats.events_denied
        );
    }
}
