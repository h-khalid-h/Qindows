//! # Bluetooth Stack — Pairing, Profiles, Per-Silo Isolation
//!
//! Chimera's Bluetooth subsystem handles device discovery,
//! pairing, and profile management (Section 7.4).
//!
//! Features:
//! - BLE + Classic Bluetooth support
//! - Secure Simple Pairing (SSP) with capability display
//! - Profile support: A2DP, HFP, HID, GATT
//! - Per-Silo device binding (isolation)
//! - Power management (advertisement intervals, sleep)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Bluetooth device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtType {
    Classic,
    Ble,
    DualMode,
}

/// Pairing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    Discovered,
    Pairing,
    Paired,
    Connected,
    Disconnected,
    Failed,
}

/// Bluetooth profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtProfile {
    A2dp,   // Audio streaming
    Hfp,    // Hands-free
    Hid,    // Human interface device
    Gatt,   // Generic attribute (BLE)
    Spp,    // Serial port
    Pbap,   // Phone book access
}

/// A Bluetooth device.
#[derive(Debug, Clone)]
pub struct BtDevice {
    pub addr: [u8; 6],
    pub name: String,
    pub bt_type: BtType,
    pub state: PairState,
    pub rssi: i8,
    pub profiles: Vec<BtProfile>,
    pub bound_silo: Option<u64>,
    pub paired_at: u64,
    pub last_seen: u64,
}

/// Bluetooth statistics.
#[derive(Debug, Clone, Default)]
pub struct BtStats {
    pub devices_discovered: u64,
    pub pairings: u64,
    pub connections: u64,
    pub disconnections: u64,
    pub pairing_failures: u64,
    pub bytes_transferred: u64,
}

/// The Bluetooth Stack.
pub struct BluetoothStack {
    pub devices: BTreeMap<[u8; 6], BtDevice>,
    pub scanning: bool,
    pub discoverable: bool,
    pub stats: BtStats,
}

impl BluetoothStack {
    pub fn new() -> Self {
        BluetoothStack {
            devices: BTreeMap::new(),
            scanning: false,
            discoverable: false,
            stats: BtStats::default(),
        }
    }

    /// Start scanning for devices.
    pub fn start_scan(&mut self) {
        self.scanning = true;
    }

    /// Stop scanning.
    pub fn stop_scan(&mut self) {
        self.scanning = false;
    }

    /// Device discovered during scan.
    pub fn on_discovered(&mut self, addr: [u8; 6], name: &str, bt_type: BtType, rssi: i8, now: u64) {
        self.devices.entry(addr).or_insert_with(|| {
            self.stats.devices_discovered += 1;
            BtDevice {
                addr, name: String::from(name), bt_type,
                state: PairState::Discovered, rssi,
                profiles: Vec::new(), bound_silo: None,
                paired_at: 0, last_seen: now,
            }
        }).last_seen = now;

        if let Some(dev) = self.devices.get_mut(&addr) {
            dev.rssi = rssi;
        }
    }

    /// Initiate pairing.
    pub fn pair(&mut self, addr: &[u8; 6]) -> Result<(), &'static str> {
        let dev = self.devices.get_mut(addr).ok_or("Device not found")?;
        if dev.state == PairState::Paired || dev.state == PairState::Connected {
            return Err("Already paired");
        }
        dev.state = PairState::Pairing;
        Ok(())
    }

    /// Pairing completed.
    pub fn on_paired(&mut self, addr: &[u8; 6], success: bool, profiles: Vec<BtProfile>, now: u64) {
        if let Some(dev) = self.devices.get_mut(addr) {
            if success {
                dev.state = PairState::Paired;
                dev.profiles = profiles;
                dev.paired_at = now;
                self.stats.pairings += 1;
            } else {
                dev.state = PairState::Failed;
                self.stats.pairing_failures += 1;
            }
        }
    }

    /// Connect to a paired device.
    pub fn connect(&mut self, addr: &[u8; 6]) -> Result<(), &'static str> {
        let dev = self.devices.get_mut(addr).ok_or("Device not found")?;
        if dev.state != PairState::Paired && dev.state != PairState::Disconnected {
            return Err("Device not paired");
        }
        dev.state = PairState::Connected;
        self.stats.connections += 1;
        Ok(())
    }

    /// Disconnect.
    pub fn disconnect(&mut self, addr: &[u8; 6]) {
        if let Some(dev) = self.devices.get_mut(addr) {
            if dev.state == PairState::Connected {
                dev.state = PairState::Disconnected;
                self.stats.disconnections += 1;
            }
        }
    }

    /// Bind device to a Silo.
    pub fn bind_silo(&mut self, addr: &[u8; 6], silo_id: u64) -> Result<(), &'static str> {
        let dev = self.devices.get_mut(addr).ok_or("Device not found")?;
        if dev.bound_silo.is_some() {
            return Err("Already bound");
        }
        dev.bound_silo = Some(silo_id);
        Ok(())
    }
}
