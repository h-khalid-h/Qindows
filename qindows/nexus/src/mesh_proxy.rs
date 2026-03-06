//! # Mesh Proxy — Reverse Proxy + Load Balancer for Mesh Services
//!
//! Routes mesh service requests through a local reverse proxy
//! with load balancing and health checking (Section 11.15).
//!
//! Features:
//! - Per-service backend pools
//! - Round-robin and least-connections balancing
//! - Health check probing
//! - Request routing by path prefix
//! - Per-Silo proxy rules

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Load balancing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LbStrategy {
    RoundRobin,
    LeastConnections,
    Random,
}

/// Backend health state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Down,
    Unknown,
}

/// A backend server.
#[derive(Debug, Clone)]
pub struct Backend {
    pub id: u64,
    pub node_id: [u8; 32],
    pub port: u16,
    pub health: HealthState,
    pub active_connections: u32,
    pub total_requests: u64,
    pub last_health_check: u64,
    pub weight: u8,
}

/// A proxy route.
#[derive(Debug, Clone)]
pub struct ProxyRoute {
    pub id: u64,
    pub path_prefix: String,
    pub backends: Vec<u64>,
    pub strategy: LbStrategy,
    pub silo_id: u64,
    pub rr_index: usize,
}

/// Proxy statistics.
#[derive(Debug, Clone, Default)]
pub struct ProxyStats {
    pub requests_routed: u64,
    pub backends_added: u64,
    pub health_checks: u64,
    pub backends_down: u64,
}

/// The Mesh Proxy.
pub struct MeshProxy {
    pub backends: BTreeMap<u64, Backend>,
    pub routes: BTreeMap<u64, ProxyRoute>,
    next_backend_id: u64,
    next_route_id: u64,
    pub health_interval_ms: u64,
    pub stats: ProxyStats,
}

impl MeshProxy {
    pub fn new() -> Self {
        MeshProxy {
            backends: BTreeMap::new(),
            routes: BTreeMap::new(),
            next_backend_id: 1,
            next_route_id: 1,
            health_interval_ms: 10_000,
            stats: ProxyStats::default(),
        }
    }

    /// Add a backend server.
    pub fn add_backend(&mut self, node_id: [u8; 32], port: u16, weight: u8) -> u64 {
        let id = self.next_backend_id;
        self.next_backend_id += 1;
        self.backends.insert(id, Backend {
            id, node_id, port, health: HealthState::Unknown,
            active_connections: 0, total_requests: 0,
            last_health_check: 0, weight,
        });
        self.stats.backends_added += 1;
        id
    }

    /// Create a proxy route.
    pub fn add_route(&mut self, prefix: &str, backend_ids: Vec<u64>, strategy: LbStrategy, silo_id: u64) -> u64 {
        let id = self.next_route_id;
        self.next_route_id += 1;
        self.routes.insert(id, ProxyRoute {
            id, path_prefix: String::from(prefix),
            backends: backend_ids, strategy, silo_id, rr_index: 0,
        });
        id
    }

    /// Route a request — returns the selected backend ID.
    pub fn route(&mut self, path: &str, silo_id: u64) -> Option<u64> {
        // Find matching route
        let route_id = self.routes.values()
            .filter(|r| r.silo_id == silo_id && path.starts_with(&r.path_prefix))
            .max_by_key(|r| r.path_prefix.len())
            .map(|r| r.id)?;

        let route = self.routes.get_mut(&route_id)?;

        // Filter healthy backends
        let healthy: Vec<u64> = route.backends.iter()
            .filter(|&&id| {
                self.backends.get(&id)
                    .map(|b| b.health != HealthState::Down)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        if healthy.is_empty() { return None; }

        let selected = match route.strategy {
            LbStrategy::RoundRobin => {
                let idx = route.rr_index % healthy.len();
                route.rr_index += 1;
                healthy[idx]
            }
            LbStrategy::LeastConnections => {
                *healthy.iter()
                    .min_by_key(|&&id| {
                        self.backends.get(&id).map(|b| b.active_connections).unwrap_or(u32::MAX)
                    })
                    .unwrap()
            }
            LbStrategy::Random => {
                healthy[route.rr_index % healthy.len()] // Simplified
            }
        };

        if let Some(b) = self.backends.get_mut(&selected) {
            b.active_connections += 1;
            b.total_requests += 1;
        }
        self.stats.requests_routed += 1;
        Some(selected)
    }

    /// Update backend health.
    pub fn update_health(&mut self, backend_id: u64, health: HealthState, now: u64) {
        if let Some(b) = self.backends.get_mut(&backend_id) {
            if health == HealthState::Down && b.health != HealthState::Down {
                self.stats.backends_down += 1;
            }
            b.health = health;
            b.last_health_check = now;
        }
        self.stats.health_checks += 1;
    }
}
