//! # Mesh Metrics — Distributed Telemetry Aggregation
//!
//! Collects, aggregates, and queries metrics from mesh
//! nodes for observability (Section 11.16).
//!
//! Features:
//! - Counter, gauge, histogram metric types
//! - Per-node metric streams
//! - Time-windowed aggregation
//! - Tag-based filtering
//! - Top-K queries

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

/// A metric data point.
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub timestamp: u64,
    pub value: f64,
    pub node_id: [u8; 32],
}

/// A metric series.
#[derive(Debug, Clone)]
pub struct MetricSeries {
    pub name: String,
    pub mtype: MetricType,
    pub tags: BTreeMap<String, String>,
    pub points: Vec<DataPoint>,
    pub max_points: usize,
}

/// Aggregated result.
#[derive(Debug, Clone)]
pub struct AggResult {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// Metrics statistics.
#[derive(Debug, Clone, Default)]
pub struct MetricsStats {
    pub series_created: u64,
    pub points_ingested: u64,
    pub queries: u64,
}

/// The Mesh Metrics Engine.
pub struct MeshMetrics {
    pub series: BTreeMap<String, MetricSeries>,
    pub stats: MetricsStats,
    pub default_max_points: usize,
}

impl MeshMetrics {
    pub fn new() -> Self {
        MeshMetrics {
            series: BTreeMap::new(),
            stats: MetricsStats::default(),
            default_max_points: 10_000,
        }
    }

    /// Create or get a metric series.
    pub fn register(&mut self, name: &str, mtype: MetricType, tags: BTreeMap<String, String>) {
        if !self.series.contains_key(name) {
            self.series.insert(String::from(name), MetricSeries {
                name: String::from(name), mtype, tags,
                points: Vec::new(), max_points: self.default_max_points,
            });
            self.stats.series_created += 1;
        }
    }

    /// Ingest a data point.
    pub fn ingest(&mut self, name: &str, value: f64, node_id: [u8; 32], timestamp: u64) {
        if let Some(series) = self.series.get_mut(name) {
            series.points.push(DataPoint { timestamp, value, node_id });
            self.stats.points_ingested += 1;

            // Evict oldest if over limit
            if series.points.len() > series.max_points {
                series.points.remove(0);
            }
        }
    }

    /// Query aggregated metrics for a time window.
    pub fn aggregate(&mut self, name: &str, start: u64, end: u64) -> Option<AggResult> {
        let series = self.series.get(name)?;
        self.stats.queries += 1;

        let filtered: Vec<f64> = series.points.iter()
            .filter(|p| p.timestamp >= start && p.timestamp <= end)
            .map(|p| p.value)
            .collect();

        if filtered.is_empty() { return None; }

        let count = filtered.len() as u64;
        let sum: f64 = filtered.iter().sum();
        let min = filtered.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = filtered.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        Some(AggResult {
            count, sum, min, max,
            avg: sum / count as f64,
        })
    }

    /// Get top-K series by latest value.
    pub fn top_k(&self, k: usize) -> Vec<(&str, f64)> {
        let mut latest: Vec<(&str, f64)> = self.series.values()
            .filter_map(|s| s.points.last().map(|p| (s.name.as_str(), p.value)))
            .collect();
        latest.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        latest.truncate(k);
        latest
    }
}
