//! # PCM Audio Silo Cap Bridge (Phase 232)
//!
//! ## Architecture Guardian: The Gap
//! `pcm_audio.rs` implements `PcmMixer`:
//! - `create_stream(silo_id, sample_rate, channels, format)` → Option<stream_id>
//! - `submit(stream_id, samples: &[f32])` — submit audio samples
//! - `mix(output: &mut [f32])` — mix all streams to output
//!
//! **Missing link**: `create_stream()` placed no limit on streams per Silo.
//! A Silo could create thousands of streams, exhausting mixer memory and
//! blocking audio output for all other Silos.
//!
//! This module provides `PcmAudioSiloCapBridge`:
//! Max 4 audio streams per Silo.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::pcm_audio::{PcmMixer, SampleFormat};

const MAX_STREAMS_PER_SILO: u32 = 4;

#[derive(Debug, Default, Clone)]
pub struct PcmAudioCapStats {
    pub streams_created:  u64,
    pub streams_denied:   u64,
}

pub struct PcmAudioSiloCapBridge {
    pub mixer:      PcmMixer,
    silo_streams:   BTreeMap<u64, u32>,
    pub stats:      PcmAudioCapStats,
}

impl PcmAudioSiloCapBridge {
    pub fn new(output_rate: u32, output_channels: u8) -> Self {
        PcmAudioSiloCapBridge { mixer: PcmMixer::new(output_rate, output_channels), silo_streams: BTreeMap::new(), stats: PcmAudioCapStats::default() }
    }

    pub fn create_stream(
        &mut self,
        silo_id: u64,
        sample_rate: u32,
        channels: u8,
        format: SampleFormat,
    ) -> Option<u64> {
        let count = *self.silo_streams.get(&silo_id).unwrap_or(&0);
        if count >= MAX_STREAMS_PER_SILO {
            self.stats.streams_denied += 1;
            crate::serial_println!("[PCM] Silo {} stream quota exceeded ({}/{})", silo_id, count, MAX_STREAMS_PER_SILO);
            return None;
        }
        let sid = self.mixer.create_stream(silo_id, sample_rate, channels, format)?;
        *self.silo_streams.entry(silo_id).or_default() += 1;
        self.stats.streams_created += 1;
        Some(sid)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PcmAudioBridge: created={} denied={}",
            self.stats.streams_created, self.stats.streams_denied
        );
    }
}
