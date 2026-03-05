//! # Nexus Bandwidth Monitor
//!
//! Real-time bandwidth monitoring per network interface and Silo.
//! Tracks throughput, packet counts, errors, and rolling averages.
//! Feeds data into Qernel telemetry for dashboards and alerts.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── Interface Stats ────────────────────────────────────────────────────────

/// A network interface.
#[derive(Debug, Clone)]
pub struct Interface {
    /// Interface name (e.g., "eth0", "wlan0", "mesh0")
    pub name: String,
    /// Is the interface up?
    pub up: bool,
    /// Link speed in Mbps (0 = unknown)
    pub speed_mbps: u32,
    /// MAC address
    pub mac: [u8; 6],
    /// IPv4 address (if assigned)
    pub ipv4: Option<[u8; 4]>,
}

/// Cumulative counters for an interface.
#[derive(Debug, Clone, Default)]
pub struct InterfaceCounters {
    /// Bytes received
    pub rx_bytes: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Packets transmitted
    pub tx_packets: u64,
    /// Receive errors
    pub rx_errors: u64,
    /// Transmit errors
    pub tx_errors: u64,
    /// Packets dropped (rx)
    pub rx_drops: u64,
    /// Packets dropped (tx)
    pub tx_drops: u64,
}

/// A bandwidth sample (snapshot of rates).
#[derive(Debug, Clone, Copy)]
pub struct BandwidthSample {
    /// Timestamp (ns)
    pub timestamp: u64,
    /// Receive rate (bytes/sec)
    pub rx_bps: u64,
    /// Transmit rate (bytes/sec)
    pub tx_bps: u64,
    /// Receive packets/sec
    pub rx_pps: u64,
    /// Transmit packets/sec
    pub tx_pps: u64,
}

// ─── Per-Silo Tracking ─────────────────────────────────────────────────────

/// Per-Silo bandwidth accounting.
#[derive(Debug, Clone, Default)]
pub struct SiloBandwidth {
    /// Silo ID
    pub silo_id: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_recv: u64,
    /// Current send rate (bytes/sec)
    pub send_rate: u64,
    /// Current receive rate (bytes/sec)
    pub recv_rate: u64,
    /// Active connections
    pub connections: u32,
    /// Bandwidth quota (bytes/sec, 0 = unlimited)
    pub quota_bps: u64,
    /// Is this Silo over quota?
    pub over_quota: bool,
}

// ─── Bandwidth Alerts ───────────────────────────────────────────────────────

/// Bandwidth alert type.
#[derive(Debug, Clone)]
pub enum BandwidthAlert {
    /// Interface utilization exceeded threshold
    HighUtilization { interface: String, percent: f32 },
    /// Silo exceeded its bandwidth quota
    QuotaExceeded { silo_id: u64, rate: u64, quota: u64 },
    /// Interface errors spike
    ErrorSpike { interface: String, errors_per_sec: u64 },
    /// Interface went down
    LinkDown { interface: String },
    /// Interface came up
    LinkUp { interface: String },
}

// ─── Bandwidth Monitor ─────────────────────────────────────────────────────

/// Monitor statistics.
#[derive(Debug, Clone, Default)]
pub struct MonitorStats {
    pub samples_collected: u64,
    pub alerts_generated: u64,
    pub interfaces_tracked: u64,
    pub silos_tracked: u64,
}

/// The Bandwidth Monitor.
pub struct BandwidthMonitor {
    /// Tracked interfaces
    pub interfaces: BTreeMap<String, Interface>,
    /// Current counters per interface
    pub counters: BTreeMap<String, InterfaceCounters>,
    /// Previous counters (for rate calculation)
    prev_counters: BTreeMap<String, InterfaceCounters>,
    /// Last sample timestamp
    prev_timestamp: u64,
    /// Bandwidth history per interface (ring buffer)
    pub history: BTreeMap<String, Vec<BandwidthSample>>,
    /// History capacity per interface
    pub history_capacity: usize,
    /// Per-Silo accounting
    pub silo_bandwidth: BTreeMap<u64, SiloBandwidth>,
    /// Previous Silo counters (for rate calculation)
    prev_silo: BTreeMap<u64, (u64, u64)>,
    /// Alert threshold: utilization percent (0-100)
    pub alert_utilization_pct: f32,
    /// Alert threshold: errors per second
    pub alert_errors_per_sec: u64,
    /// Statistics
    pub stats: MonitorStats,
}

impl BandwidthMonitor {
    pub fn new() -> Self {
        BandwidthMonitor {
            interfaces: BTreeMap::new(),
            counters: BTreeMap::new(),
            prev_counters: BTreeMap::new(),
            prev_timestamp: 0,
            history: BTreeMap::new(),
            history_capacity: 3600, // 1 hour at 1 sample/sec
            silo_bandwidth: BTreeMap::new(),
            prev_silo: BTreeMap::new(),
            alert_utilization_pct: 80.0,
            alert_errors_per_sec: 100,
            stats: MonitorStats::default(),
        }
    }

    /// Register a network interface.
    pub fn add_interface(&mut self, iface: Interface) {
        let name = iface.name.clone();
        self.interfaces.insert(name.clone(), iface);
        self.counters.insert(name.clone(), InterfaceCounters::default());
        self.history.insert(name, Vec::new());
        self.stats.interfaces_tracked += 1;
    }

    /// Update raw counters for an interface (called from driver).
    pub fn update_counters(&mut self, name: &str, new_counters: InterfaceCounters) {
        if let Some(counters) = self.counters.get_mut(name) {
            *counters = new_counters;
        }
    }

    /// Record Silo network activity.
    pub fn record_silo_traffic(
        &mut self,
        silo_id: u64,
        bytes_sent: u64,
        bytes_recv: u64,
        connections: u32,
    ) {
        let entry = self.silo_bandwidth.entry(silo_id)
            .or_insert_with(|| {
                self.stats.silos_tracked += 1;
                SiloBandwidth { silo_id, ..Default::default() }
            });

        entry.bytes_sent += bytes_sent;
        entry.bytes_recv += bytes_recv;
        entry.connections = connections;
    }

    /// Take a sample: compute rates from counter deltas.
    pub fn sample(&mut self, now: u64) -> Vec<BandwidthAlert> {
        self.stats.samples_collected += 1;
        let mut alerts = Vec::new();

        let elapsed_ns = now.saturating_sub(self.prev_timestamp);
        if elapsed_ns == 0 { return alerts; }
        let elapsed_sec = elapsed_ns as f64 / 1_000_000_000.0;

        // Per-interface rates
        for (name, current) in &self.counters {
            let prev = self.prev_counters.entry(name.clone())
                .or_insert_with(InterfaceCounters::default);

            let rx_bytes_delta = current.rx_bytes.saturating_sub(prev.rx_bytes);
            let tx_bytes_delta = current.tx_bytes.saturating_sub(prev.tx_bytes);
            let rx_pkts_delta = current.rx_packets.saturating_sub(prev.rx_packets);
            let tx_pkts_delta = current.tx_packets.saturating_sub(prev.tx_packets);
            let errors_delta = (current.rx_errors + current.tx_errors)
                .saturating_sub(prev.rx_errors + prev.tx_errors);

            let sample = BandwidthSample {
                timestamp: now,
                rx_bps: (rx_bytes_delta as f64 / elapsed_sec) as u64,
                tx_bps: (tx_bytes_delta as f64 / elapsed_sec) as u64,
                rx_pps: (rx_pkts_delta as f64 / elapsed_sec) as u64,
                tx_pps: (tx_pkts_delta as f64 / elapsed_sec) as u64,
            };

            // Store in history
            if let Some(hist) = self.history.get_mut(name) {
                if hist.len() >= self.history_capacity {
                    hist.remove(0);
                }
                hist.push(sample);
            }

            // Check utilization alert
            if let Some(iface) = self.interfaces.get(name) {
                if iface.speed_mbps > 0 {
                    let max_bps = iface.speed_mbps as u64 * 125_000; // Mbps to bytes/sec
                    let total_bps = sample.rx_bps + sample.tx_bps;
                    let utilization = (total_bps * 100) as f32 / max_bps as f32;
                    if utilization > self.alert_utilization_pct {
                        alerts.push(BandwidthAlert::HighUtilization {
                            interface: name.clone(),
                            percent: utilization,
                        });
                    }
                }
            }

            // Check error spike
            let errors_per_sec = (errors_delta as f64 / elapsed_sec) as u64;
            if errors_per_sec > self.alert_errors_per_sec {
                alerts.push(BandwidthAlert::ErrorSpike {
                    interface: name.clone(),
                    errors_per_sec,
                });
            }
        }

        // Update previous counters
        self.prev_counters = self.counters.clone();

        // Per-Silo rates
        for (silo_id, bw) in &mut self.silo_bandwidth {
            let prev = self.prev_silo.entry(*silo_id).or_insert((0, 0));
            let sent_delta = bw.bytes_sent.saturating_sub(prev.0);
            let recv_delta = bw.bytes_recv.saturating_sub(prev.1);

            bw.send_rate = (sent_delta as f64 / elapsed_sec) as u64;
            bw.recv_rate = (recv_delta as f64 / elapsed_sec) as u64;

            // Check quota
            if bw.quota_bps > 0 {
                let total_rate = bw.send_rate + bw.recv_rate;
                bw.over_quota = total_rate > bw.quota_bps;
                if bw.over_quota {
                    alerts.push(BandwidthAlert::QuotaExceeded {
                        silo_id: *silo_id,
                        rate: total_rate,
                        quota: bw.quota_bps,
                    });
                }
            }

            *prev = (bw.bytes_sent, bw.bytes_recv);
        }

        self.prev_timestamp = now;
        self.stats.alerts_generated += alerts.len() as u64;
        alerts
    }

    /// Get the latest bandwidth sample for an interface.
    pub fn latest(&self, name: &str) -> Option<&BandwidthSample> {
        self.history.get(name)?.last()
    }

    /// Get average throughput over the last N samples.
    pub fn average(&self, name: &str, n: usize) -> Option<(u64, u64)> {
        let hist = self.history.get(name)?;
        let count = n.min(hist.len());
        if count == 0 { return None; }

        let start = hist.len() - count;
        let (rx_sum, tx_sum) = hist[start..].iter()
            .fold((0u64, 0u64), |(rx, tx), s| (rx + s.rx_bps, tx + s.tx_bps));

        Some((rx_sum / count as u64, tx_sum / count as u64))
    }

    /// Set a bandwidth quota for a Silo.
    pub fn set_silo_quota(&mut self, silo_id: u64, quota_bps: u64) {
        let entry = self.silo_bandwidth.entry(silo_id)
            .or_insert_with(|| SiloBandwidth { silo_id, ..Default::default() });
        entry.quota_bps = quota_bps;
    }

    /// Get top Silos by bandwidth usage.
    pub fn top_silos(&self, n: usize) -> Vec<&SiloBandwidth> {
        let mut silos: Vec<&SiloBandwidth> = self.silo_bandwidth.values().collect();
        silos.sort_by(|a, b| {
            let a_total = a.send_rate + a.recv_rate;
            let b_total = b.send_rate + b.recv_rate;
            b_total.cmp(&a_total)
        });
        silos.truncate(n);
        silos
    }
}
