//! # Mesh Monitor — Mesh Health Dashboard + Alerts
//!
//! Monitors mesh node health, connectivity, latency, and
//! resource usage. Triggers alerts on anomalies (Section 11.10).
//!
//! Features:
//! - Node heartbeat tracking
//! - Latency measurement between nodes
//! - Resource usage (CPU, memory, storage) polling
//! - Alert thresholds per metric
//! - Node health scoring

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Node health level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthLevel {
    Healthy,
    Warning,
    Critical,
    Unreachable,
}

/// Alert severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// A mesh node status.
#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub node_id: [u8; 32],
    pub name: String,
    pub health: HealthLevel,
    pub last_heartbeat: u64,
    pub latency_ms: u32,
    pub cpu_percent: u8,
    pub memory_percent: u8,
    pub storage_percent: u8,
    pub uptime_secs: u64,
}

/// A monitoring alert.
#[derive(Debug, Clone)]
pub struct Alert {
    pub id: u64,
    pub node_id: [u8; 32],
    pub severity: Severity,
    pub message: String,
    pub timestamp: u64,
    pub acknowledged: bool,
}

/// Monitor statistics.
#[derive(Debug, Clone, Default)]
pub struct MonitorStats {
    pub heartbeats_received: u64,
    pub alerts_fired: u64,
    pub nodes_unreachable: u64,
    pub checks_performed: u64,
}

/// The Mesh Monitor.
pub struct MeshMonitor {
    pub nodes: BTreeMap<[u8; 32], NodeStatus>,
    pub alerts: Vec<Alert>,
    next_alert_id: u64,
    pub heartbeat_timeout_ms: u64,
    pub cpu_warn_threshold: u8,
    pub memory_warn_threshold: u8,
    pub stats: MonitorStats,
}

impl MeshMonitor {
    pub fn new() -> Self {
        MeshMonitor {
            nodes: BTreeMap::new(),
            alerts: Vec::new(),
            next_alert_id: 1,
            heartbeat_timeout_ms: 30_000,
            cpu_warn_threshold: 90,
            memory_warn_threshold: 85,
            stats: MonitorStats::default(),
        }
    }

    /// Record a heartbeat from a node.
    pub fn heartbeat(&mut self, node_id: [u8; 32], name: &str, cpu: u8, mem: u8, storage: u8, latency_ms: u32, uptime: u64, now: u64) {
        let status = self.nodes.entry(node_id).or_insert(NodeStatus {
            node_id, name: String::from(name), health: HealthLevel::Healthy,
            last_heartbeat: 0, latency_ms: 0, cpu_percent: 0,
            memory_percent: 0, storage_percent: 0, uptime_secs: 0,
        });

        status.last_heartbeat = now;
        status.latency_ms = latency_ms;
        status.cpu_percent = cpu;
        status.memory_percent = mem;
        status.storage_percent = storage;
        status.uptime_secs = uptime;
        status.health = HealthLevel::Healthy;
        self.stats.heartbeats_received += 1;
    }

    /// Check all nodes for health issues.
    pub fn check(&mut self, now: u64) {
        self.stats.checks_performed += 1;

        // Collect node data first to avoid borrow conflicts
        let node_data: Vec<([u8; 32], String, u64, u8, u8)> = self.nodes.values()
            .map(|n| (n.node_id, n.name.clone(), n.last_heartbeat, n.cpu_percent, n.memory_percent))
            .collect();

        for (node_id, name, last_hb, cpu, mem) in node_data {
            // Heartbeat timeout
            if now.saturating_sub(last_hb) > self.heartbeat_timeout_ms {
                if let Some(n) = self.nodes.get_mut(&node_id) {
                    if n.health != HealthLevel::Unreachable {
                        n.health = HealthLevel::Unreachable;
                        self.stats.nodes_unreachable += 1;
                        self.fire_alert(node_id, Severity::Critical,
                            &alloc::format!("Node {} unreachable", name), now);
                    }
                }
                continue;
            }

            // CPU threshold
            if cpu >= self.cpu_warn_threshold {
                if let Some(n) = self.nodes.get_mut(&node_id) {
                    if n.health == HealthLevel::Healthy {
                        n.health = HealthLevel::Warning;
                        self.fire_alert(node_id, Severity::Warning,
                            &alloc::format!("Node {} CPU at {}%", name, cpu), now);
                    }
                }
            }

            // Memory threshold
            if mem >= self.memory_warn_threshold {
                if let Some(n) = self.nodes.get_mut(&node_id) {
                    if n.health == HealthLevel::Healthy {
                        n.health = HealthLevel::Warning;
                        self.fire_alert(node_id, Severity::Warning,
                            &alloc::format!("Node {} memory at {}%", name, mem), now);
                    }
                }
            }
        }
    }

    fn fire_alert(&mut self, node_id: [u8; 32], severity: Severity, message: &str, now: u64) {
        let id = self.next_alert_id;
        self.next_alert_id += 1;
        self.alerts.push(Alert {
            id, node_id, severity, message: String::from(message),
            timestamp: now, acknowledged: false,
        });
        self.stats.alerts_fired += 1;
    }

    /// Acknowledge an alert.
    pub fn acknowledge(&mut self, alert_id: u64) {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == alert_id) {
            alert.acknowledged = true;
        }
    }

    /// Get unacknowledged alerts.
    pub fn pending_alerts(&self) -> Vec<&Alert> {
        self.alerts.iter().filter(|a| !a.acknowledged).collect()
    }
}
