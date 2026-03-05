//! # Qernel Device Hotplug Manager
//!
//! Manages runtime device add/remove events across PCI, USB,
//! CPU cores, and memory DIMMs. Maintains a live device census
//! and dispatches events to registered handlers per bus type.
//!
//! Integrates with `pci_scan` for PCI enumeration and `usb`
//! for xHCI port status change events.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Bus Types ──────────────────────────────────────────────────────────────

/// Bus/subsystem that generated the hotplug event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugBus {
    /// PCI/PCIe device
    Pci,
    /// USB device (via xHCI port status change)
    Usb,
    /// CPU core (online/offline)
    Cpu,
    /// Memory DIMM (hot-add/remove)
    Memory,
    /// Thunderbolt / external enclosure
    Thunderbolt,
    /// NVMe namespace (hot-add)
    Nvme,
}

/// Hotplug event direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugAction {
    /// Device was attached / came online
    Add,
    /// Device was removed / went offline
    Remove,
    /// Device requests safe-removal (eject button pressed)
    EjectRequest,
    /// Device encountered a surprise removal (yanked without eject)
    SurpriseRemoval,
    /// Device reset (e.g. PCIe FLR)
    Reset,
}

// ─── Events ─────────────────────────────────────────────────────────────────

/// A hotplug event.
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    /// Event serial number
    pub id: u64,
    /// Which bus generated this event
    pub bus: HotplugBus,
    /// Add or remove
    pub action: HotplugAction,
    /// Bus-specific location (PCI BDF, USB port, core ID, etc.)
    pub location: DeviceLocation,
    /// Device identity (vendor:product or description)
    pub identity: String,
    /// Timestamp (ns since boot)
    pub timestamp: u64,
    /// Was the event handled successfully?
    pub handled: bool,
}

/// Bus-specific addressing.
#[derive(Debug, Clone)]
pub enum DeviceLocation {
    /// PCI: bus, device, function
    Pci { bus: u8, device: u8, function: u8 },
    /// USB: controller, port
    Usb { controller: u8, port: u8 },
    /// CPU core index
    Cpu(u32),
    /// Memory: NUMA node, DIMM slot
    Memory { node: u8, slot: u8 },
    /// NVMe: controller, namespace
    Nvme { controller: u8, namespace: u32 },
    /// Generic (Thunderbolt, etc.)
    Other(u64),
}

// ─── Policy ─────────────────────────────────────────────────────────────────

/// What to do when a new device appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugPolicy {
    /// Auto-configure and make available immediately
    AutoConfigure,
    /// Notify the user and wait for approval
    NotifyOnly,
    /// Deny the device (security-critical environments)
    Deny,
    /// Auto-configure but quarantine (sandboxed driver)
    Quarantine,
}

/// Per-bus policy entry.
#[derive(Debug, Clone)]
pub struct PolicyEntry {
    /// Bus this policy applies to
    pub bus: HotplugBus,
    /// Default policy for this bus
    pub default_policy: HotplugPolicy,
    /// Vendor:Product allowlist (empty = allow all)
    pub allowlist: Vec<String>,
    /// Vendor:Product denylist
    pub denylist: Vec<String>,
}

// ─── Device Census ──────────────────────────────────────────────────────────

/// A known device in the census.
#[derive(Debug, Clone)]
pub struct CensusEntry {
    /// Unique entry ID
    pub id: u64,
    /// Bus type
    pub bus: HotplugBus,
    /// Location on the bus
    pub location: DeviceLocation,
    /// Device identity
    pub identity: String,
    /// When the device was first seen
    pub first_seen: u64,
    /// Is the device currently present?
    pub present: bool,
    /// Assigned driver name (if any)
    pub driver: Option<String>,
    /// Has the device been ejected safely?
    pub ejected: bool,
}

// ─── Manager ────────────────────────────────────────────────────────────────

/// Hotplug statistics.
#[derive(Debug, Clone, Default)]
pub struct HotplugStats {
    pub events_received: u64,
    pub devices_added: u64,
    pub devices_removed: u64,
    pub surprise_removals: u64,
    pub policy_denials: u64,
    pub auto_configured: u64,
}

/// The Hotplug Manager.
pub struct HotplugManager {
    /// Pending event queue
    pub events: Vec<HotplugEvent>,
    /// Device census (all known devices)
    pub census: Vec<CensusEntry>,
    /// Per-bus policies
    pub policies: Vec<PolicyEntry>,
    /// Next event ID
    next_event_id: u64,
    /// Next census ID
    next_census_id: u64,
    /// Statistics
    pub stats: HotplugStats,
}

impl HotplugManager {
    pub fn new() -> Self {
        let policies = alloc::vec![
            PolicyEntry {
                bus: HotplugBus::Pci,
                default_policy: HotplugPolicy::AutoConfigure,
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            PolicyEntry {
                bus: HotplugBus::Usb,
                default_policy: HotplugPolicy::AutoConfigure,
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            PolicyEntry {
                bus: HotplugBus::Cpu,
                default_policy: HotplugPolicy::AutoConfigure,
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            PolicyEntry {
                bus: HotplugBus::Memory,
                default_policy: HotplugPolicy::NotifyOnly,
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            PolicyEntry {
                bus: HotplugBus::Nvme,
                default_policy: HotplugPolicy::AutoConfigure,
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
        ];

        HotplugManager {
            events: Vec::new(),
            census: Vec::new(),
            policies,
            next_event_id: 1,
            next_census_id: 1,
            stats: HotplugStats::default(),
        }
    }

    /// Submit a hotplug event.
    pub fn submit_event(
        &mut self,
        bus: HotplugBus,
        action: HotplugAction,
        location: DeviceLocation,
        identity: &str,
        now: u64,
    ) -> u64 {
        let id = self.next_event_id;
        self.next_event_id += 1;
        self.stats.events_received += 1;

        let event = HotplugEvent {
            id,
            bus,
            action,
            location: location.clone(),
            identity: String::from(identity),
            timestamp: now,
            handled: false,
        };

        crate::serial_println!(
            "[HOTPLUG] {:?} {:?} on {:?}: {}",
            action, bus, location, identity
        );

        self.events.push(event);

        // Process immediately
        self.process_event(id, location, bus, action, identity, now);

        id
    }

    /// Process a single event against policy.
    fn process_event(
        &mut self,
        event_id: u64,
        location: DeviceLocation,
        bus: HotplugBus,
        action: HotplugAction,
        identity: &str,
        now: u64,
    ) {
        match action {
            HotplugAction::Add => {
                let policy = self.evaluate_policy(bus, identity);

                match policy {
                    HotplugPolicy::Deny => {
                        self.stats.policy_denials += 1;
                        crate::serial_println!("[HOTPLUG] DENIED: {}", identity);
                    }
                    HotplugPolicy::AutoConfigure | HotplugPolicy::Quarantine => {
                        self.add_to_census(bus, location, identity, now);
                        self.stats.devices_added += 1;
                        self.stats.auto_configured += 1;
                    }
                    HotplugPolicy::NotifyOnly => {
                        self.add_to_census(bus, location, identity, now);
                        self.stats.devices_added += 1;
                    }
                }
            }
            HotplugAction::Remove | HotplugAction::EjectRequest => {
                self.mark_removed(bus, identity);
                self.stats.devices_removed += 1;
            }
            HotplugAction::SurpriseRemoval => {
                self.mark_removed(bus, identity);
                self.stats.devices_removed += 1;
                self.stats.surprise_removals += 1;
            }
            HotplugAction::Reset => {
                // Device reset — driver needs to re-initialize
            }
        }

        // Mark event as handled
        if let Some(evt) = self.events.iter_mut().find(|e| e.id == event_id) {
            evt.handled = true;
        }
    }

    /// Evaluate the policy for a device.
    fn evaluate_policy(&self, bus: HotplugBus, identity: &str) -> HotplugPolicy {
        if let Some(policy) = self.policies.iter().find(|p| p.bus == bus) {
            // Check denylist first
            if policy.denylist.iter().any(|d| identity.contains(d.as_str())) {
                return HotplugPolicy::Deny;
            }
            // If allowlist is non-empty, device must be on it
            if !policy.allowlist.is_empty()
                && !policy.allowlist.iter().any(|a| identity.contains(a.as_str()))
            {
                return HotplugPolicy::Deny;
            }
            policy.default_policy
        } else {
            HotplugPolicy::NotifyOnly
        }
    }

    /// Add a device to the census.
    fn add_to_census(
        &mut self,
        bus: HotplugBus,
        location: DeviceLocation,
        identity: &str,
        now: u64,
    ) {
        // Check if it already exists and is not present (re-attach)
        if let Some(entry) = self.census.iter_mut().find(|e| e.identity == identity && !e.present) {
            entry.present = true;
            return;
        }

        let id = self.next_census_id;
        self.next_census_id += 1;

        self.census.push(CensusEntry {
            id,
            bus,
            location,
            identity: String::from(identity),
            first_seen: now,
            present: true,
            driver: None,
            ejected: false,
        });
    }

    /// Mark a device as removed.
    fn mark_removed(&mut self, bus: HotplugBus, identity: &str) {
        if let Some(entry) = self.census.iter_mut().find(|e| {
            e.bus == bus && e.identity == identity && e.present
        }) {
            entry.present = false;
        }
    }

    /// Safe-eject a device by census ID.
    pub fn safe_eject(&mut self, census_id: u64) -> bool {
        if let Some(entry) = self.census.iter_mut().find(|e| e.id == census_id && e.present) {
            entry.ejected = true;
            entry.present = false;
            true
        } else {
            false
        }
    }

    /// Get all currently present devices.
    pub fn present_devices(&self) -> Vec<&CensusEntry> {
        self.census.iter().filter(|e| e.present).collect()
    }

    /// Get pending (unhandled) events.
    pub fn pending_events(&self) -> Vec<&HotplugEvent> {
        self.events.iter().filter(|e| !e.handled).collect()
    }

    /// Set policy for a bus type.
    pub fn set_policy(&mut self, bus: HotplugBus, policy: HotplugPolicy) {
        if let Some(entry) = self.policies.iter_mut().find(|p| p.bus == bus) {
            entry.default_policy = policy;
        }
    }

    /// Add a device identity to a bus's denylist.
    pub fn deny_device(&mut self, bus: HotplugBus, identity: &str) {
        if let Some(entry) = self.policies.iter_mut().find(|p| p.bus == bus) {
            entry.denylist.push(String::from(identity));
        }
    }
}
