//! # Audio Mixer — Per-Silo Audio Streams, Volume, Routing
//!
//! Manages audio output mixing for multiple Silos with
//! independent volume controls and routing (Section 4.9).
//!
//! Features:
//! - Per-Silo audio streams with independent volume
//! - Master volume + per-stream volume
//! - Output device routing (speakers, headphones, Bluetooth)
//! - Ducking (lower background audio when priority stream plays)
//! - Mute per Silo

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Audio stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Playing,
    Paused,
    Stopped,
    Ducked,
}

/// Stream priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamPriority {
    Background = 0,
    Media = 1,
    Communication = 2,
    System = 3,
    Emergency = 4,
}

/// An audio stream.
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub id: u64,
    pub silo_id: u64,
    pub name: String,
    pub state: StreamState,
    pub priority: StreamPriority,
    pub volume: f32,      // 0.0 - 1.0
    pub muted: bool,
    pub output_device: u32,
    pub sample_rate: u32,
    pub channels: u8,
}

/// An output device.
#[derive(Debug, Clone)]
pub struct OutputDevice {
    pub id: u32,
    pub name: String,
    pub active: bool,
    pub max_channels: u8,
}

/// Mixer statistics.
#[derive(Debug, Clone, Default)]
pub struct MixerStats {
    pub streams_created: u64,
    pub streams_destroyed: u64,
    pub duck_events: u64,
    pub samples_mixed: u64,
}

/// The Audio Mixer.
pub struct AudioMixer {
    pub streams: BTreeMap<u64, AudioStream>,
    pub devices: BTreeMap<u32, OutputDevice>,
    pub master_volume: f32,
    pub master_mute: bool,
    next_stream_id: u64,
    pub duck_factor: f32,
    pub stats: MixerStats,
}

impl AudioMixer {
    pub fn new() -> Self {
        AudioMixer {
            streams: BTreeMap::new(),
            devices: BTreeMap::new(),
            master_volume: 1.0,
            master_mute: false,
            next_stream_id: 1,
            duck_factor: 0.3,
            stats: MixerStats::default(),
        }
    }

    /// Register an output device.
    pub fn add_device(&mut self, id: u32, name: &str, max_ch: u8) {
        self.devices.insert(id, OutputDevice {
            id, name: String::from(name), active: true, max_channels: max_ch,
        });
    }

    /// Create an audio stream.
    pub fn create_stream(&mut self, silo_id: u64, name: &str, priority: StreamPriority, device: u32, rate: u32, channels: u8) -> u64 {
        let id = self.next_stream_id;
        self.next_stream_id += 1;

        self.streams.insert(id, AudioStream {
            id, silo_id, name: String::from(name),
            state: StreamState::Stopped, priority,
            volume: 1.0, muted: false, output_device: device,
            sample_rate: rate, channels,
        });

        self.stats.streams_created += 1;
        id
    }

    /// Play a stream.
    pub fn play(&mut self, stream_id: u64) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = StreamState::Playing;
            let priority = stream.priority;

            // Duck lower-priority streams
            let to_duck: Vec<u64> = self.streams.iter()
                .filter(|(sid, s)| **sid != stream_id && s.state == StreamState::Playing && s.priority < priority)
                .map(|(sid, _)| *sid)
                .collect();

            for sid in to_duck {
                if let Some(s) = self.streams.get_mut(&sid) {
                    s.state = StreamState::Ducked;
                    self.stats.duck_events += 1;
                }
            }
        }
    }

    /// Pause a stream.
    pub fn pause(&mut self, stream_id: u64) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = StreamState::Paused;
        }
    }

    /// Stop and remove a stream.
    pub fn stop(&mut self, stream_id: u64) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = StreamState::Stopped;
        }
        self.stats.streams_destroyed += 1;
    }

    /// Set stream volume.
    pub fn set_volume(&mut self, stream_id: u64, vol: f32) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.volume = vol.max(0.0).min(1.0);
        }
    }

    /// Set master volume.
    pub fn set_master_volume(&mut self, vol: f32) {
        self.master_volume = vol.max(0.0).min(1.0);
    }

    /// Mute a Silo's streams.
    pub fn mute_silo(&mut self, silo_id: u64) {
        for stream in self.streams.values_mut() {
            if stream.silo_id == silo_id {
                stream.muted = true;
            }
        }
    }

    /// Get effective volume for a stream.
    pub fn effective_volume(&self, stream_id: u64) -> f32 {
        if self.master_mute { return 0.0; }
        match self.streams.get(&stream_id) {
            Some(s) if s.muted => 0.0,
            Some(s) if s.state == StreamState::Ducked => s.volume * self.master_volume * self.duck_factor,
            Some(s) => s.volume * self.master_volume,
            None => 0.0,
        }
    }
}
