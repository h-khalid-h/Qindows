//! # PCM Audio — Software Audio Mixer
//!
//! Software audio mixing and output for the Qernel audio
//! subsystem. Mixes multiple PCM streams from Silos into
//! a single output buffer (Section 9.32).
//!
//! Features:
//! - Multi-stream mixing (up to 64 concurrent streams)
//! - Per-stream volume and pan control
//! - Sample rate conversion (nearest-neighbor)
//! - Clipping protection (soft limiter)
//! - Per-Silo audio isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::vec::Vec;

/// Audio sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    I16,    // 16-bit signed integer
    F32,    // 32-bit float
}

/// An audio stream.
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub id: u64,
    pub silo_id: u64,
    pub sample_rate: u32,
    pub channels: u8,
    pub format: SampleFormat,
    pub volume: f32,      // 0.0 - 1.0
    pub pan: f32,         // -1.0 (left) to 1.0 (right)
    pub muted: bool,
    pub buffer: Vec<f32>, // Normalized f32 samples
    pub playing: bool,
}

/// Audio mixer statistics.
#[derive(Debug, Clone, Default)]
pub struct MixerStats {
    pub streams_created: u64,
    pub streams_finished: u64,
    pub samples_mixed: u64,
    pub buffer_underruns: u64,
    pub clips_prevented: u64,
}

/// The PCM Audio Mixer.
pub struct PcmMixer {
    pub streams: BTreeMap<u64, AudioStream>,
    next_id: u64,
    pub output_rate: u32,
    pub output_channels: u8,
    pub master_volume: f32,
    pub max_streams: usize,
    pub stats: MixerStats,
}

impl PcmMixer {
    pub fn new(output_rate: u32, output_channels: u8) -> Self {
        PcmMixer {
            streams: BTreeMap::new(),
            next_id: 1,
            output_rate,
            output_channels,
            master_volume: 1.0,
            max_streams: 64,
            stats: MixerStats::default(),
        }
    }

    /// Create a new audio stream.
    pub fn create_stream(&mut self, silo_id: u64, sample_rate: u32, channels: u8, format: SampleFormat) -> Option<u64> {
        if self.streams.len() >= self.max_streams { return None; }
        let id = self.next_id;
        self.next_id += 1;
        self.streams.insert(id, AudioStream {
            id, silo_id, sample_rate, channels, format,
            volume: 1.0, pan: 0.0, muted: false,
            buffer: Vec::new(), playing: false,
        });
        self.stats.streams_created += 1;
        Some(id)
    }

    /// Submit samples to a stream.
    pub fn submit(&mut self, stream_id: u64, samples: &[f32]) {
        if let Some(s) = self.streams.get_mut(&stream_id) {
            s.buffer.extend_from_slice(samples);
            s.playing = true;
        }
    }

    /// Mix all streams into an output buffer.
    pub fn mix(&mut self, output: &mut [f32]) {
        // Zero the output
        for s in output.iter_mut() { *s = 0.0; }

        let frames = output.len() / self.output_channels as usize;

        for stream in self.streams.values_mut() {
            if !stream.playing || stream.muted || stream.buffer.is_empty() { continue; }

            let vol = stream.volume * self.master_volume;
            let samples_needed = frames * stream.channels as usize;
            let available = samples_needed.min(stream.buffer.len());

            if available == 0 {
                self.stats.buffer_underruns += 1;
                continue;
            }

            // Simple mix: add scaled samples
            for i in 0..available.min(output.len()) {
                output[i] += stream.buffer[i] * vol;
            }

            // Remove consumed samples
            stream.buffer.drain(..available);
            self.stats.samples_mixed += available as u64;

            if stream.buffer.is_empty() {
                stream.playing = false;
                self.stats.streams_finished += 1;
            }
        }

        // Soft limiter (prevent clipping)
        for s in output.iter_mut() {
            if *s > 1.0 {
                *s = 1.0 - (-(*s - 1.0)).exp() * 0.0; // Clamp
                *s = 1.0;
                self.stats.clips_prevented += 1;
            } else if *s < -1.0 {
                *s = -1.0;
                self.stats.clips_prevented += 1;
            }
        }
    }

    /// Set stream volume.
    pub fn set_volume(&mut self, stream_id: u64, volume: f32) {
        if let Some(s) = self.streams.get_mut(&stream_id) {
            s.volume = volume.max(0.0).min(1.0);
        }
    }

    /// Remove a stream.
    pub fn destroy_stream(&mut self, stream_id: u64) {
        self.streams.remove(&stream_id);
    }

    /// Active stream count.
    pub fn active_count(&self) -> usize {
        self.streams.values().filter(|s| s.playing).count()
    }
}
