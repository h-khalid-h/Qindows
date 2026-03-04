//! # Nexus Service Discovery
//!
//! Discover and register services on the Global Mesh.
//! Each node can advertise services (file sharing, compute,
//! relay, storage) and discover services offered by peers.
//! Built on top of the Kademlia DHT.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// A service type available on the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// File sharing / content delivery
    FileShare,
    /// Remote compute (offload work)
    Compute,
    /// Network relay (NAT traversal)
    Relay,
    /// Distributed storage
    Storage,
    /// Chat / messaging
    Messaging,
    /// Streaming media
    MediaStream,
    /// DNS resolution
    NameResolution,
    /// Time synchronization
    TimeSync,
    /// Custom service
    Custom(u16),
}

/// A service endpoint (address + port).
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// IP address (4 bytes IPv4)
    pub addr: [u8; 4],
    /// Port number
    pub port: u16,
    /// Is this reachable directly (no NAT)?
    pub direct: bool,
}

/// A service advertisement.
#[derive(Debug, Clone)]
pub struct ServiceRecord {
    /// Unique service ID
    pub service_id: u64,
    /// Node ID of the provider
    pub node_id: [u8; 32],
    /// Service type
    pub service_type: ServiceType,
    /// Human-readable name
    pub name: String,
    /// Service version
    pub version: u16,
    /// Endpoints where this service is reachable
    pub endpoints: Vec<Endpoint>,
    /// Capacity (0.0 - 1.0, how much load it can handle)
    pub capacity: f32,
    /// Last heartbeat timestamp
    pub last_seen: u64,
    /// TTL in seconds (how long to cache this record)
    pub ttl: u32,
    /// Metadata key-value pairs
    pub metadata: Vec<(String, String)>,
}

/// Health status of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unreachable,
    Unknown,
}

/// A discovered service with health info.
#[derive(Debug, Clone)]
pub struct DiscoveredService {
    pub record: ServiceRecord,
    pub health: HealthStatus,
    pub latency_ms: u32,
    pub hops: u8,
}

/// The Service Discovery Engine.
pub struct ServiceDiscovery {
    /// Our node ID
    pub node_id: [u8; 32],
    /// Services we are advertising
    pub local_services: Vec<ServiceRecord>,
    /// Discovered remote services
    pub remote_services: Vec<DiscoveredService>,
    /// Next service ID
    next_id: u64,
    /// Discovery statistics
    pub stats: DiscoveryStats,
}

/// Discovery statistics.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    pub queries_sent: u64,
    pub queries_received: u64,
    pub services_registered: u64,
    pub services_discovered: u64,
    pub heartbeats_sent: u64,
}

impl ServiceDiscovery {
    pub fn new(node_id: [u8; 32]) -> Self {
        ServiceDiscovery {
            node_id,
            local_services: Vec::new(),
            remote_services: Vec::new(),
            next_id: 1,
            stats: DiscoveryStats::default(),
        }
    }

    /// Register a local service.
    pub fn register(
        &mut self,
        service_type: ServiceType,
        name: &str,
        port: u16,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let record = ServiceRecord {
            service_id: id,
            node_id: self.node_id,
            service_type,
            name: String::from(name),
            version: 1,
            endpoints: alloc::vec![Endpoint {
                addr: [0, 0, 0, 0], // Will be filled with actual IP
                port,
                direct: true,
            }],
            capacity: 1.0,
            last_seen: 0,
            ttl: 300, // 5 minutes
            metadata: Vec::new(),
        };

        self.local_services.push(record);
        self.stats.services_registered += 1;
        id
    }

    /// Unregister a local service.
    pub fn unregister(&mut self, service_id: u64) {
        self.local_services.retain(|s| s.service_id != service_id);
    }

    /// Discover services of a given type.
    pub fn discover(&mut self, service_type: ServiceType) -> Vec<&DiscoveredService> {
        self.stats.queries_sent += 1;

        self.remote_services.iter()
            .filter(|s| s.record.service_type == service_type && s.health != HealthStatus::Unreachable)
            .collect()
    }

    /// Find the best service of a given type (lowest latency, highest capacity).
    pub fn find_best(&mut self, service_type: ServiceType) -> Option<&DiscoveredService> {
        self.stats.queries_sent += 1;

        self.remote_services.iter()
            .filter(|s| s.record.service_type == service_type && s.health == HealthStatus::Healthy)
            .min_by(|a, b| {
                // Score = latency / capacity (lower is better)
                let score_a = a.latency_ms as f32 / a.record.capacity.max(0.01);
                let score_b = b.latency_ms as f32 / b.record.capacity.max(0.01);
                score_a.partial_cmp(&score_b).unwrap_or(core::cmp::Ordering::Equal)
            })
    }

    /// Process a received service advertisement.
    pub fn process_advertisement(&mut self, record: ServiceRecord, latency_ms: u32, hops: u8) {
        self.stats.services_discovered += 1;

        // Update or insert
        if let Some(existing) = self.remote_services.iter_mut()
            .find(|s| s.record.service_id == record.service_id)
        {
            existing.record = record;
            existing.latency_ms = latency_ms;
            existing.health = HealthStatus::Healthy;
        } else {
            self.remote_services.push(DiscoveredService {
                record,
                health: HealthStatus::Healthy,
                latency_ms,
                hops,
            });
        }
    }

    /// Expire stale services.
    pub fn expire(&mut self, now: u64) {
        self.remote_services.retain(|s| {
            let age = now.saturating_sub(s.record.last_seen);
            age < s.record.ttl as u64
        });
    }

    /// Get all services grouped by type.
    pub fn by_type(&self) -> alloc::collections::BTreeMap<u8, Vec<&DiscoveredService>> {
        let mut map = alloc::collections::BTreeMap::new();
        for service in &self.remote_services {
            let key = match service.record.service_type {
                ServiceType::FileShare => 0,
                ServiceType::Compute => 1,
                ServiceType::Relay => 2,
                ServiceType::Storage => 3,
                ServiceType::Messaging => 4,
                ServiceType::MediaStream => 5,
                ServiceType::NameResolution => 6,
                ServiceType::TimeSync => 7,
                ServiceType::Custom(n) => 8 + (n as u8),
            };
            map.entry(key).or_insert_with(Vec::new).push(service);
        }
        map
    }
}
