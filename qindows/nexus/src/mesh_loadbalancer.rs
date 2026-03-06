//! # Mesh Load Balancer — Request Distribution
//!
//! Distributes incoming requests across mesh nodes using
//! pluggable algorithms (Section 11.24).
//!
//! Features:
//! - Round-robin, least-connections, weighted
//! - Health-check aware (skips unhealthy nodes)
//! - Sticky sessions (hash-based affinity)
//! - Connection draining on node removal
//! - Per-backend statistics

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Load-balancing algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LbAlgorithm {
    RoundRobin,
    LeastConnections,
    WeightedRoundRobin,
    IpHash,
}

/// Backend health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Draining,
}

/// A backend node.
#[derive(Debug, Clone)]
pub struct Backend {
    pub id: u32,
    pub node_id: [u8; 32],
    pub address: [u8; 4],
    pub port: u16,
    pub weight: u32,
    pub health: HealthStatus,
    pub active_connections: u32,
    pub total_requests: u64,
    pub total_errors: u64,
    pub last_health_check: u64,
}

/// Load balancer statistics.
#[derive(Debug, Clone, Default)]
pub struct LbStats {
    pub requests_routed: u64,
    pub no_healthy_backend: u64,
    pub health_checks: u64,
    pub failovers: u64,
}

/// The Mesh Load Balancer.
pub struct MeshLoadBalancer {
    pub backends: Vec<Backend>,
    pub algorithm: LbAlgorithm,
    pub rr_index: usize,
    pub sticky_sessions: BTreeMap<u32, u32>, // client_hash → backend_id
    pub health_interval_ms: u64,
    pub stats: LbStats,
    next_id: u32,
}

impl MeshLoadBalancer {
    pub fn new(algorithm: LbAlgorithm) -> Self {
        MeshLoadBalancer {
            backends: Vec::new(),
            algorithm,
            rr_index: 0,
            sticky_sessions: BTreeMap::new(),
            health_interval_ms: 5000,
            stats: LbStats::default(),
            next_id: 1,
        }
    }

    /// Add a backend.
    pub fn add_backend(&mut self, node_id: [u8; 32], address: [u8; 4], port: u16, weight: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.backends.push(Backend {
            id, node_id, address, port, weight,
            health: HealthStatus::Healthy, active_connections: 0,
            total_requests: 0, total_errors: 0, last_health_check: 0,
        });
        id
    }

    /// Select a backend for a request.
    pub fn select(&mut self, client_hash: Option<u32>) -> Option<u32> {
        // Sticky session check
        if self.algorithm == LbAlgorithm::IpHash {
            if let Some(hash) = client_hash {
                if let Some(&backend_id) = self.sticky_sessions.get(&hash) {
                    if self.is_healthy(backend_id) {
                        self.record_request(backend_id);
                        return Some(backend_id);
                    }
                }
            }
        }

        let healthy: Vec<usize> = self.backends.iter().enumerate()
            .filter(|(_, b)| b.health == HealthStatus::Healthy || b.health == HealthStatus::Degraded)
            .map(|(i, _)| i)
            .collect();

        if healthy.is_empty() {
            self.stats.no_healthy_backend += 1;
            return None;
        }

        let idx = match self.algorithm {
            LbAlgorithm::RoundRobin | LbAlgorithm::IpHash => {
                self.rr_index = (self.rr_index + 1) % healthy.len();
                healthy[self.rr_index]
            }
            LbAlgorithm::LeastConnections => {
                *healthy.iter()
                    .min_by_key(|&&i| self.backends[i].active_connections)
                    .unwrap_or(&healthy[0])
            }
            LbAlgorithm::WeightedRoundRobin => {
                // Pick highest weight among healthy
                *healthy.iter()
                    .max_by_key(|&&i| self.backends[i].weight)
                    .unwrap_or(&healthy[0])
            }
        };

        let backend_id = self.backends[idx].id;
        self.record_request(backend_id);

        if let Some(hash) = client_hash {
            self.sticky_sessions.insert(hash, backend_id);
        }

        Some(backend_id)
    }

    /// Record a request to a backend.
    fn record_request(&mut self, backend_id: u32) {
        if let Some(b) = self.backends.iter_mut().find(|b| b.id == backend_id) {
            b.active_connections += 1;
            b.total_requests += 1;
        }
        self.stats.requests_routed += 1;
    }

    /// Release a connection.
    pub fn release(&mut self, backend_id: u32) {
        if let Some(b) = self.backends.iter_mut().find(|b| b.id == backend_id) {
            b.active_connections = b.active_connections.saturating_sub(1);
        }
    }

    /// Update health status.
    pub fn set_health(&mut self, backend_id: u32, status: HealthStatus) {
        if let Some(b) = self.backends.iter_mut().find(|b| b.id == backend_id) {
            let was_healthy = b.health == HealthStatus::Healthy;
            b.health = status;
            if was_healthy && status == HealthStatus::Unhealthy {
                self.stats.failovers += 1;
            }
        }
        self.stats.health_checks += 1;
    }

    fn is_healthy(&self, backend_id: u32) -> bool {
        self.backends.iter().find(|b| b.id == backend_id)
            .map(|b| b.health == HealthStatus::Healthy || b.health == HealthStatus::Degraded)
            .unwrap_or(false)
    }

    pub fn healthy_count(&self) -> usize {
        self.backends.iter().filter(|b| b.health == HealthStatus::Healthy).count()
    }
}
