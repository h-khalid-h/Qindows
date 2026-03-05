//! # Qernel System Telemetry
//!
//! Collects and aggregates system-wide performance metrics:
//! CPU utilization, memory pressure, I/O throughput, scheduler
//! latency, and per-Silo resource usage. Provides ring-buffer
//! time-series storage for dashboards and anomaly detection.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── Metric Types ───────────────────────────────────────────────────────────

/// Category of a metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MetricCategory {
    /// CPU-related (utilization, frequency, temperature)
    Cpu,
    /// Memory (usage, page faults, swap)
    Memory,
    /// Storage I/O (reads, writes, latency)
    Storage,
    /// Network (packets, bytes, errors)
    Network,
    /// Scheduler (context switches, queue depth)
    Scheduler,
    /// Power (watts, battery, thermal)
    Power,
    /// Per-Silo resource accounting
    Silo,
}

/// A metric data point.
#[derive(Debug, Clone, Copy)]
pub struct DataPoint {
    /// Timestamp (ns since boot)
    pub timestamp: u64,
    /// Metric value
    pub value: f64,
}

/// Aggregation over a time window.
#[derive(Debug, Clone, Copy)]
pub struct Aggregate {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub sum: f64,
    pub count: u64,
    pub window_start: u64,
    pub window_end: u64,
}

impl Default for Aggregate {
    fn default() -> Self {
        Aggregate {
            min: f64::MAX,
            max: f64::MIN,
            avg: 0.0,
            sum: 0.0,
            count: 0,
            window_start: 0,
            window_end: 0,
        }
    }
}

impl Aggregate {
    /// Add a data point to this aggregate.
    pub fn add(&mut self, value: f64) {
        if value < self.min { self.min = value; }
        if value > self.max { self.max = value; }
        self.sum += value;
        self.count += 1;
        self.avg = self.sum / self.count as f64;
    }
}

// ─── Metric Definitions ────────────────────────────────────────────────────

/// A named metric with ring-buffer storage.
pub struct Metric {
    /// Metric name (e.g., "cpu.core0.utilization")
    pub name: String,
    /// Category
    pub category: MetricCategory,
    /// Unit label (e.g., "%", "MB/s", "°C")
    pub unit: String,
    /// Ring buffer of recent data points
    pub ring: Vec<DataPoint>,
    /// Ring buffer capacity
    pub capacity: usize,
    /// Write index (wraps around)
    pub write_idx: usize,
    /// Total samples ever recorded
    pub total_samples: u64,
    /// Current aggregate (rolling window)
    pub current: Aggregate,
}

impl Metric {
    pub fn new(name: &str, category: MetricCategory, unit: &str, capacity: usize) -> Self {
        Metric {
            name: String::from(name),
            category,
            unit: String::from(unit),
            ring: Vec::new(),
            capacity,
            write_idx: 0,
            total_samples: 0,
            current: Aggregate::default(),
        }
    }

    /// Record a new data point.
    pub fn record(&mut self, value: f64, now: u64) {
        let point = DataPoint { timestamp: now, value };

        if self.ring.len() < self.capacity {
            self.ring.push(point);
        } else {
            self.ring[self.write_idx] = point;
        }
        self.write_idx = (self.write_idx + 1) % self.capacity;
        self.total_samples += 1;
        self.current.add(value);
    }

    /// Get the latest value.
    pub fn latest(&self) -> Option<f64> {
        if self.ring.is_empty() { return None; }
        let idx = if self.write_idx == 0 {
            self.ring.len() - 1
        } else {
            self.write_idx - 1
        };
        Some(self.ring[idx].value)
    }

    /// Compute aggregate over the last N samples.
    pub fn aggregate_last(&self, n: usize) -> Aggregate {
        let mut agg = Aggregate::default();
        let count = n.min(self.ring.len());
        if count == 0 { return agg; }

        let start = if self.ring.len() < self.capacity {
            self.ring.len().saturating_sub(count)
        } else {
            (self.write_idx + self.capacity - count) % self.capacity
        };

        for i in 0..count {
            let idx = (start + i) % self.ring.len();
            agg.add(self.ring[idx].value);
        }

        if !self.ring.is_empty() {
            let first_idx = start % self.ring.len();
            let last_idx = (start + count - 1) % self.ring.len();
            agg.window_start = self.ring[first_idx].timestamp;
            agg.window_end = self.ring[last_idx].timestamp;
        }

        agg
    }
}

// ─── Per-Silo Accounting ────────────────────────────────────────────────────

/// Resource usage for one Silo.
#[derive(Debug, Clone, Default)]
pub struct SiloUsage {
    /// Silo ID
    pub silo_id: u64,
    /// CPU time consumed (ns)
    pub cpu_ns: u64,
    /// Peak memory (bytes)
    pub memory_peak: u64,
    /// Current memory (bytes)
    pub memory_current: u64,
    /// Storage reads (bytes)
    pub storage_read: u64,
    /// Storage writes (bytes)
    pub storage_write: u64,
    /// Network sent (bytes)
    pub net_sent: u64,
    /// Network received (bytes)
    pub net_recv: u64,
    /// Context switches
    pub context_switches: u64,
    /// Page faults
    pub page_faults: u64,
}

// ─── Alerts ─────────────────────────────────────────────────────────────────

/// An alert threshold.
#[derive(Debug, Clone)]
pub struct AlertRule {
    /// Rule ID
    pub id: u64,
    /// Metric name to watch
    pub metric: String,
    /// Threshold value
    pub threshold: f64,
    /// Fire when above (true) or below (false)?
    pub above: bool,
    /// Consecutive violations needed before firing
    pub min_violations: u32,
    /// Current consecutive violations
    pub violations: u32,
    /// Has this alert fired?
    pub fired: bool,
}

// ─── Telemetry Engine ───────────────────────────────────────────────────────

/// Telemetry statistics.
#[derive(Debug, Clone, Default)]
pub struct TelemetryStats {
    pub total_samples: u64,
    pub metrics_registered: u64,
    pub alerts_fired: u64,
    pub snapshots_taken: u64,
}

/// The Telemetry Engine.
pub struct TelemetryEngine {
    /// All registered metrics
    pub metrics: BTreeMap<String, Metric>,
    /// Per-Silo resource accounting
    pub silo_usage: BTreeMap<u64, SiloUsage>,
    /// Alert rules
    pub alerts: Vec<AlertRule>,
    /// Next alert ID
    next_alert_id: u64,
    /// Statistics
    pub stats: TelemetryStats,
}

impl TelemetryEngine {
    pub fn new() -> Self {
        let mut engine = TelemetryEngine {
            metrics: BTreeMap::new(),
            silo_usage: BTreeMap::new(),
            alerts: Vec::new(),
            next_alert_id: 1,
            stats: TelemetryStats::default(),
        };

        // Register default system metrics
        engine.register("cpu.utilization", MetricCategory::Cpu, "%", 3600);
        engine.register("cpu.frequency", MetricCategory::Cpu, "MHz", 600);
        engine.register("cpu.temperature", MetricCategory::Cpu, "°C", 600);
        engine.register("memory.used", MetricCategory::Memory, "MB", 3600);
        engine.register("memory.page_faults", MetricCategory::Memory, "/s", 600);
        engine.register("storage.read_mbps", MetricCategory::Storage, "MB/s", 600);
        engine.register("storage.write_mbps", MetricCategory::Storage, "MB/s", 600);
        engine.register("storage.latency_us", MetricCategory::Storage, "μs", 600);
        engine.register("net.rx_mbps", MetricCategory::Network, "Mbps", 600);
        engine.register("net.tx_mbps", MetricCategory::Network, "Mbps", 600);
        engine.register("scheduler.queue_depth", MetricCategory::Scheduler, "", 600);
        engine.register("scheduler.ctx_switches", MetricCategory::Scheduler, "/s", 600);
        engine.register("power.watts", MetricCategory::Power, "W", 600);

        engine
    }

    /// Register a new metric.
    pub fn register(
        &mut self,
        name: &str,
        category: MetricCategory,
        unit: &str,
        capacity: usize,
    ) {
        let metric = Metric::new(name, category, unit, capacity);
        self.metrics.insert(String::from(name), metric);
        self.stats.metrics_registered += 1;
    }

    /// Record a metric value.
    pub fn record(&mut self, name: &str, value: f64, now: u64) {
        if let Some(metric) = self.metrics.get_mut(name) {
            metric.record(value, now);
            self.stats.total_samples += 1;
        }
    }

    /// Record Silo resource usage.
    pub fn record_silo(
        &mut self,
        silo_id: u64,
        cpu_ns: u64,
        memory: u64,
        storage_r: u64,
        storage_w: u64,
    ) {
        let usage = self.silo_usage.entry(silo_id).or_insert_with(|| {
            SiloUsage { silo_id, ..Default::default() }
        });

        usage.cpu_ns += cpu_ns;
        usage.memory_current = memory;
        if memory > usage.memory_peak {
            usage.memory_peak = memory;
        }
        usage.storage_read += storage_r;
        usage.storage_write += storage_w;
    }

    /// Add an alert rule.
    pub fn add_alert(
        &mut self,
        metric: &str,
        threshold: f64,
        above: bool,
        min_violations: u32,
    ) -> u64 {
        let id = self.next_alert_id;
        self.next_alert_id += 1;

        self.alerts.push(AlertRule {
            id,
            metric: String::from(metric),
            threshold,
            above,
            min_violations,
            violations: 0,
            fired: false,
        });

        id
    }

    /// Check all alert rules against current metric values.
    pub fn check_alerts(&mut self) -> Vec<u64> {
        let mut fired = Vec::new();

        for alert in &mut self.alerts {
            if alert.fired { continue; }

            let current = self.metrics.get(&alert.metric)
                .and_then(|m| m.latest());

            if let Some(val) = current {
                let violated = if alert.above {
                    val > alert.threshold
                } else {
                    val < alert.threshold
                };

                if violated {
                    alert.violations += 1;
                    if alert.violations >= alert.min_violations {
                        alert.fired = true;
                        fired.push(alert.id);
                    }
                } else {
                    alert.violations = 0;
                }
            }
        }

        self.stats.alerts_fired += fired.len() as u64;
        fired
    }

    /// Get a snapshot of all latest metric values.
    pub fn snapshot(&mut self) -> Vec<(String, f64)> {
        self.stats.snapshots_taken += 1;
        self.metrics.iter()
            .filter_map(|(name, m)| m.latest().map(|v| (name.clone(), v)))
            .collect()
    }

    /// Get metrics by category.
    pub fn by_category(&self, cat: MetricCategory) -> Vec<(&str, f64)> {
        self.metrics.iter()
            .filter(|(_, m)| m.category == cat)
            .filter_map(|(name, m)| m.latest().map(|v| (name.as_str(), v)))
            .collect()
    }

    /// Get top N Silos by CPU usage.
    pub fn top_silos_by_cpu(&self, n: usize) -> Vec<&SiloUsage> {
        let mut silos: Vec<&SiloUsage> = self.silo_usage.values().collect();
        silos.sort_by(|a, b| b.cpu_ns.cmp(&a.cpu_ns));
        silos.truncate(n);
        silos
    }
}
