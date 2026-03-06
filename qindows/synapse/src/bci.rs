//! # BCI — Brain-Computer Interface Hardware Abstraction
//!
//! Abstracts over BCI hardware devices (EEG headsets, neural
//! implants) and provides a unified stream of neural samples
//! to the Synapse processing pipeline (Section 6.2).
//!
//! Features:
//! - Device enumeration and capability detection
//! - Channel configuration (sampling rate, electrode mapping)
//! - Raw sample streaming with ring buffer
//! - Signal quality monitoring
//! - Per-Silo device binding

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// BCI device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BciDeviceType {
    Eeg,           // Surface EEG headset
    Ecog,          // Electrocorticography
    Implant,       // Neural implant (Neuralink-style)
    Fnirs,         // Functional near-infrared spectroscopy
    Emg,           // Electromyography (muscle)
}

/// BCI device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Disconnected,
    Connecting,
    Calibrating,
    Active,
    Error,
}

/// A BCI device.
#[derive(Debug, Clone)]
pub struct BciDevice {
    pub id: u64,
    pub name: String,
    pub device_type: BciDeviceType,
    pub state: DeviceState,
    pub channels: u16,
    pub sample_rate_hz: u32,
    pub resolution_bits: u8,
    pub bound_silo: Option<u64>,
}

/// A raw neural sample from the device.
#[derive(Debug, Clone)]
pub struct RawSample {
    pub device_id: u64,
    pub timestamp_us: u64,
    pub channel_data: Vec<f32>,
    pub quality: f32, // 0.0–1.0
}

/// BCI statistics.
#[derive(Debug, Clone, Default)]
pub struct BciStats {
    pub devices_connected: u64,
    pub samples_received: u64,
    pub samples_dropped: u64,
    pub calibrations: u64,
}

/// The BCI Manager.
pub struct BciManager {
    pub devices: BTreeMap<u64, BciDevice>,
    /// Ring buffer of recent samples per device
    pub sample_buffers: BTreeMap<u64, Vec<RawSample>>,
    pub buffer_size: usize,
    next_id: u64,
    pub stats: BciStats,
}

impl BciManager {
    pub fn new(buffer_size: usize) -> Self {
        BciManager {
            devices: BTreeMap::new(),
            sample_buffers: BTreeMap::new(),
            buffer_size,
            next_id: 1,
            stats: BciStats::default(),
        }
    }

    /// Register a BCI device.
    pub fn connect(&mut self, name: &str, device_type: BciDeviceType, channels: u16, rate: u32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.devices.insert(id, BciDevice {
            id, name: String::from(name), device_type,
            state: DeviceState::Connecting, channels,
            sample_rate_hz: rate, resolution_bits: 24,
            bound_silo: None,
        });
        self.sample_buffers.insert(id, Vec::new());
        self.stats.devices_connected += 1;
        id
    }

    /// Push a sample into the ring buffer.
    pub fn push_sample(&mut self, sample: RawSample) {
        let dev_id = sample.device_id;
        if let Some(buf) = self.sample_buffers.get_mut(&dev_id) {
            if buf.len() >= self.buffer_size {
                buf.remove(0);
                self.stats.samples_dropped += 1;
            }
            buf.push(sample);
            self.stats.samples_received += 1;
        }
    }

    /// Drain samples for processing.
    pub fn drain(&mut self, device_id: u64) -> Vec<RawSample> {
        self.sample_buffers.get_mut(&device_id)
            .map(|buf| core::mem::take(buf))
            .unwrap_or_default()
    }

    /// Set device state.
    pub fn set_state(&mut self, device_id: u64, state: DeviceState) {
        if let Some(dev) = self.devices.get_mut(&device_id) {
            dev.state = state;
        }
    }

    /// Bind device to a Silo.
    pub fn bind_silo(&mut self, device_id: u64, silo_id: u64) {
        if let Some(dev) = self.devices.get_mut(&device_id) {
            dev.bound_silo = Some(silo_id);
        }
    }
}
