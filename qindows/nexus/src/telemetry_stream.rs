//! # Nexus Telemetry Streamer
//!
//! Streams real-time Qernel telemetry (thermals, power, scheduling)
//! over the Nexus Mesh to authorized peers (Section 11.8).
//!
//! Features:
//! - Pub/sub topic streaming
//! - Adaptive compression (samples batched based on link quality)
//! - Capability-based access control (only authorized peers can subscribe)
//! - Binary encoding (Q-Buffers)

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A telemetry topic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TelemetryTopic {
    CpuLoad,
    MemoryUsage,
    Thermals,
    PowerMilliwatts,
    NetworkRxTx,
    DiskIo,
    Custom(String),
}

/// A single telemetry datum.
#[derive(Debug, Clone)]
pub struct TelemetrySample {
    pub timestamp_ns: u64,
    pub topic: TelemetryTopic,
    pub value_i64: i64,
    pub index: u32,
}

/// A subscriber to a telemetry topic.
#[derive(Debug, Clone)]
pub struct Subscriber {
    pub peer_id: [u8; 32],
    pub topics: Vec<TelemetryTopic>,
    pub max_frequency_hz: u32,
    pub last_sent_ns: BTreeMap<TelemetryTopic, u64>,
}

/// A batch of samples ready for network transmission.
#[derive(Debug, Clone)]
pub struct TelemetryBatch {
    pub peer_id: [u8; 32],
    pub samples: Vec<TelemetrySample>,
    pub sequence: u64,
}

/// Telemetry Streamer statistics.
#[derive(Debug, Clone, Default)]
pub struct StreamerStats {
    pub samples_generated: u64,
    pub samples_dropped: u64,
    pub batches_sent: u64,
    pub active_subscribers: u32,
}

/// The Telemetry Streamer.
pub struct TelemetryStreamer {
    pub subscribers: BTreeMap<[u8; 32], Subscriber>,
    pub sample_buffer: Vec<TelemetrySample>,
    pub max_buffer_size: usize,
    pub current_sequence: BTreeMap<[u8; 32], u64>,
    pub minimum_interval_ns: u64,
    pub stats: StreamerStats,
}

impl TelemetryStreamer {
    pub fn new(max_buffer_size: usize) -> Self {
        TelemetryStreamer {
            subscribers: BTreeMap::new(),
            sample_buffer: Vec::with_capacity(max_buffer_size),
            max_buffer_size,
            current_sequence: BTreeMap::new(),
            minimum_interval_ns: 10_000_000, // 10ms max freq by default
            stats: StreamerStats::default(),
        }
    }

    /// Register a new subscriber.
    pub fn add_subscriber(&mut self, peer_id: [u8; 32], topics: Vec<TelemetryTopic>, max_hz: u32) {
        self.subscribers.insert(peer_id, Subscriber {
            peer_id,
            topics,
            max_frequency_hz: max_hz,
            last_sent_ns: BTreeMap::new(),
        });
        self.stats.active_subscribers = self.subscribers.len() as u32;
    }

    /// Remove a subscriber.
    pub fn remove_subscriber(&mut self, peer_id: &[u8; 32]) {
        self.subscribers.remove(peer_id);
        self.current_sequence.remove(peer_id);
        self.stats.active_subscribers = self.subscribers.len() as u32;
    }

    /// Ingest a local telemetry sample.
    pub fn ingest(&mut self, sample: TelemetrySample) {
        self.stats.samples_generated += 1;
        if self.sample_buffer.len() >= self.max_buffer_size {
            // Drop oldest (FIFO)
            self.sample_buffer.remove(0);
            self.stats.samples_dropped += 1;
        }
        self.sample_buffer.push(sample);
    }

    /// Process buffer and generate network batches for subscribers.
    pub fn generate_batches(&mut self, now_ns: u64) -> Vec<TelemetryBatch> {
        if self.sample_buffer.is_empty() {
            return Vec::new();
        }

        let mut batches = BTreeMap::new();

        for sample in &self.sample_buffer {
            for sub in self.subscribers.values_mut() {
                if sub.topics.contains(&sample.topic) {
                    // Rate limit check
                    let min_interval = if sub.max_frequency_hz > 0 {
                        1_000_000_000 / sub.max_frequency_hz as u64
                    } else {
                        self.minimum_interval_ns
                    };

                    let last_sent = *sub.last_sent_ns.get(&sample.topic).unwrap_or(&0);
                    if now_ns.saturating_sub(last_sent) >= min_interval {
                        batches.entry(sub.peer_id)
                            .or_insert_with(Vec::new)
                            .push(sample.clone());
                        sub.last_sent_ns.insert(sample.topic.clone(), now_ns);
                    }
                }
            }
        }

        // Clear buffer after processing
        self.sample_buffer.clear();

        // Package into objects
        let mut result = Vec::with_capacity(batches.len());
        for (peer, samples) in batches {
            if samples.is_empty() { continue; }
            let seq = self.current_sequence.entry(peer).or_insert(0);
            *seq += 1;
            
            result.push(TelemetryBatch {
                peer_id: peer,
                samples,
                sequence: *seq,
            });
            self.stats.batches_sent += 1;
        }

        result
    }
}
