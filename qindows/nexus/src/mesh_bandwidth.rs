//! # Mesh Bandwidth Monitor — Link Capacity Tracking
//!
//! Monitors bandwidth usage across mesh links and provides
//! congestion signals to the load balancer (Section 11.28).
//!
//! Features:
//! - Per-link throughput measurement
//! - Sliding window bandwidth estimation
//! - Congestion detection (utilization thresholds)
//! - Quality of Service (QoS) class tracking
//! - Historical bandwidth graphs

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// QoS traffic class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QosClass {
    BestEffort = 0,
    Background = 1,
    Interactive = 2,
    RealTime = 3,
    Control = 4,
}

/// A single bandwidth sample.
#[derive(Debug, Clone, Copy)]
pub struct BwSample {
    pub timestamp: u64,
    pub bytes: u64,
    pub packets: u32,
}

/// Per-link bandwidth state.
#[derive(Debug, Clone)]
pub struct LinkBandwidth {
    pub peer_id: [u8; 32],
    pub capacity_bps: u64,
    pub samples: Vec<BwSample>,
    pub max_samples: usize,
    pub current_bps: u64,
    pub peak_bps: u64,
    pub total_bytes: u64,
    pub congested: bool,
    pub congestion_threshold_pct: u8,
}

impl LinkBandwidth {
    pub fn new(peer_id: [u8; 32], capacity_bps: u64) -> Self {
        LinkBandwidth {
            peer_id, capacity_bps,
            samples: Vec::new(), max_samples: 60,
            current_bps: 0, peak_bps: 0, total_bytes: 0,
            congested: false, congestion_threshold_pct: 80,
        }
    }

    pub fn utilization_pct(&self) -> f64 {
        if self.capacity_bps == 0 { return 0.0; }
        (self.current_bps as f64 / self.capacity_bps as f64) * 100.0
    }

    pub fn record(&mut self, bytes: u64, packets: u32, now: u64) {
        self.samples.push(BwSample { timestamp: now, bytes, packets });
        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
        self.total_bytes += bytes;
        self.recalculate();
    }

    fn recalculate(&mut self) {
        if self.samples.len() < 2 { self.current_bps = 0; return; }
        let first = self.samples.first().unwrap();
        let last = self.samples.last().unwrap();
        let duration_s = last.timestamp.saturating_sub(first.timestamp) as f64 / 1000.0;
        if duration_s < 0.001 { return; }
        let total: u64 = self.samples.iter().map(|s| s.bytes).sum();
        self.current_bps = (total as f64 * 8.0 / duration_s) as u64;
        if self.current_bps > self.peak_bps { self.peak_bps = self.current_bps; }
        self.congested = self.utilization_pct() > self.congestion_threshold_pct as f64;
    }
}

/// Bandwidth monitor statistics.
#[derive(Debug, Clone, Default)]
pub struct BwStats {
    pub samples_recorded: u64,
    pub congestion_events: u64,
    pub links_monitored: u64,
}

/// The Mesh Bandwidth Monitor.
pub struct MeshBandwidth {
    pub links: BTreeMap<[u8; 32], LinkBandwidth>,
    pub per_class_bytes: BTreeMap<QosClass, u64>,
    pub stats: BwStats,
}

impl MeshBandwidth {
    pub fn new() -> Self {
        MeshBandwidth {
            links: BTreeMap::new(),
            per_class_bytes: BTreeMap::new(),
            stats: BwStats::default(),
        }
    }

    /// Register a link.
    pub fn add_link(&mut self, peer_id: [u8; 32], capacity_bps: u64) {
        self.links.insert(peer_id, LinkBandwidth::new(peer_id, capacity_bps));
        self.stats.links_monitored += 1;
    }

    /// Record traffic on a link.
    pub fn record(&mut self, peer_id: &[u8; 32], bytes: u64, packets: u32, class: QosClass, now: u64) {
        if let Some(link) = self.links.get_mut(peer_id) {
            let was_congested = link.congested;
            link.record(bytes, packets, now);
            if !was_congested && link.congested {
                self.stats.congestion_events += 1;
            }
        }
        *self.per_class_bytes.entry(class).or_insert(0) += bytes;
        self.stats.samples_recorded += 1;
    }

    /// Get congested links.
    pub fn congested_links(&self) -> Vec<&LinkBandwidth> {
        self.links.values().filter(|l| l.congested).collect()
    }
}
