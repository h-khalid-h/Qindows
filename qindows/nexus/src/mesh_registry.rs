//! # Mesh Service Registry
//!
//! A distributed service registry for Q-Mesh nodes. Services
//! register endpoints and capabilities; clients discover and
//! connect to services by name, version, and tags (Section 11.8).
//!
//! Features:
//! - Service registration with health status
//! - Service discovery by name/version/tags
//! - Lease-based TTL (services must re-register)
//! - Load-balanced endpoint selection
//! - Per-Silo service isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Service health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// A service endpoint.
#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    pub node_id: [u8; 32],
    pub port: u16,
    pub weight: u32,
    pub health: ServiceHealth,
    pub load: u8, // 0-100
}

/// A registered service.
#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub version: u32,
    pub silo_id: Option<u64>,
    pub endpoints: Vec<ServiceEndpoint>,
    pub tags: Vec<String>,
    pub registered_at: u64,
    pub lease_ttl: u64,
    pub lease_expires: u64,
}

impl Service {
    /// Get healthy endpoints sorted by load.
    pub fn healthy_endpoints(&self) -> Vec<&ServiceEndpoint> {
        let mut eps: Vec<&ServiceEndpoint> = self.endpoints.iter()
            .filter(|e| e.health == ServiceHealth::Healthy)
            .collect();
        eps.sort_by_key(|e| e.load);
        eps
    }
}

/// Registry statistics.
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    pub services_registered: u64,
    pub services_expired: u64,
    pub lookups: u64,
    pub discoveries: u64,
}

/// The Service Registry.
pub struct MeshRegistry {
    /// name → service
    pub services: BTreeMap<String, Vec<Service>>,
    pub stats: RegistryStats,
}

impl MeshRegistry {
    pub fn new() -> Self {
        MeshRegistry {
            services: BTreeMap::new(),
            stats: RegistryStats::default(),
        }
    }

    /// Register a service.
    pub fn register(
        &mut self, name: &str, version: u32, silo_id: Option<u64>,
        endpoint: ServiceEndpoint, tags: &[&str],
        ttl: u64, now: u64,
    ) {
        let svc_list = self.services.entry(String::from(name))
            .or_insert_with(Vec::new);

        // Check if this exact version+node already exists
        let existing = svc_list.iter_mut().find(|s| {
            s.version == version && s.silo_id == silo_id
        });

        if let Some(svc) = existing {
            // Update endpoint + renew lease
            if let Some(ep) = svc.endpoints.iter_mut()
                .find(|e| e.node_id == endpoint.node_id) {
                ep.port = endpoint.port;
                ep.weight = endpoint.weight;
                ep.health = endpoint.health;
                ep.load = endpoint.load;
            } else {
                svc.endpoints.push(endpoint);
            }
            svc.lease_expires = now + ttl;
        } else {
            svc_list.push(Service {
                name: String::from(name), version, silo_id,
                endpoints: alloc::vec![endpoint],
                tags: tags.iter().map(|t| String::from(*t)).collect(),
                registered_at: now, lease_ttl: ttl,
                lease_expires: now + ttl,
            });
            self.stats.services_registered += 1;
        }
    }

    /// Discover a service by name.
    pub fn discover(&mut self, name: &str) -> Vec<&Service> {
        self.stats.lookups += 1;
        match self.services.get(name) {
            Some(svcs) => {
                self.stats.discoveries += 1;
                svcs.iter().collect()
            }
            None => Vec::new(),
        }
    }

    /// Discover with tag filter.
    pub fn discover_tagged(&mut self, name: &str, required_tag: &str) -> Vec<&Service> {
        self.stats.lookups += 1;
        match self.services.get(name) {
            Some(svcs) => svcs.iter()
                .filter(|s| s.tags.iter().any(|t| t == required_tag))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Expire services whose leases have lapsed.
    pub fn expire(&mut self, now: u64) -> u32 {
        let mut expired = 0u32;
        for svcs in self.services.values_mut() {
            let before = svcs.len();
            svcs.retain(|s| s.lease_expires > now);
            expired += (before - svcs.len()) as u32;
        }
        self.stats.services_expired += expired as u64;
        expired
    }
}
