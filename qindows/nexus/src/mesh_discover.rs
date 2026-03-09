//! # Mesh Discovery Coordinator
//!
//! Bridges the mDNS Responder (`mdns.rs`) and the DHT-based
//! Service Discovery (`discovery.rs`) into a unified discovery
//! system for the Global Mesh.
//!
//! ## Lifecycle
//!
//! 1. **Boot** — auto-registers core Qindows services on mDNS
//! 2. **Tick** — drains mDNS events → feeds into ServiceDiscovery
//! 3. **Browse** — unified search across mDNS + DHT
//! 4. **Shutdown** — sends mDNS goodbye packets

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

use crate::mdns::{MdnsResponder, MdnsEvent, DiscoveredService as MdnsDiscovered};
use crate::discovery::{ServiceDiscovery, ServiceType, ServiceRecord, Endpoint};

/// Core Qindows service definitions for mDNS auto-registration.
const QINDOWS_SERVICES: &[(&str, &str, u16)] = &[
    ("Q-Shell",  "_qshell._tcp",  7700),
    ("Prism",    "_prism._tcp",   7701),
    ("Aether",   "_aether._tcp",  7702),
    ("Nexus",    "_nexus._tcp",   7703),
    ("Sentinel", "_sentinel._tcp", 7704),
];

/// The mDNS mesh service type used for peer discovery.
const MESH_SERVICE_TYPE: &str = "_qindows._tcp";

/// Combined mesh discovery statistics.
#[derive(Debug, Clone, Default)]
pub struct MeshDiscoveryStats {
    /// Total mDNS queries answered
    pub mdns_queries_answered: u64,
    /// Total mDNS services discovered
    pub mdns_services_found: u64,
    /// Total DHT services discovered
    pub dht_services_found: u64,
    /// Total services currently known
    pub active_services: usize,
    /// Boot ticks elapsed
    pub uptime_ticks: u64,
}

/// The Mesh Discovery Coordinator.
///
/// Unifies mDNS zero-conf (LAN) and DHT-based (WAN) service discovery
/// into a single event-driven system.
pub struct MeshDiscovery {
    /// RFC 6762 mDNS responder (LAN discovery)
    pub mdns: MdnsResponder,
    /// Kademlia DHT service registry (WAN discovery)
    pub discovery: ServiceDiscovery,
    /// Our hostname
    pub hostname: String,
    /// Our IPv4 address
    pub address: [u8; 4],
    /// Node identity (32-byte public key)
    pub node_id: [u8; 32],
    /// Whether boot registration is complete
    booted: bool,
    /// Statistics
    pub stats: MeshDiscoveryStats,
}

impl MeshDiscovery {
    /// Create a new mesh discovery coordinator.
    pub fn new(hostname: &str, address: [u8; 4], node_id: [u8; 32]) -> Self {
        MeshDiscovery {
            mdns: MdnsResponder::new(hostname, address),
            discovery: ServiceDiscovery::new(node_id),
            hostname: String::from(hostname),
            address,
            node_id,
            booted: false,
            stats: MeshDiscoveryStats::default(),
        }
    }

    /// Boot the mesh discovery system.
    ///
    /// Auto-registers all core Qindows services on both mDNS and DHT.
    /// Called once during kernel Phase 14 (Service Silos).
    pub fn boot(&mut self, silo_id: u64) {
        if self.booted { return; }

        // Register the mesh peer itself
        self.mdns.register_service(
            &self.hostname,
            MESH_SERVICE_TYPE,
            7700,
            vec![
                (String::from("version"), String::from("1.0.0-genesis")),
                (String::from("arch"), String::from("x86_64")),
                (String::from("os"), String::from("qindows")),
            ],
            silo_id,
        );

        // Register each core service on mDNS
        for &(instance, svc_type, port) in QINDOWS_SERVICES {
            self.mdns.register_service(
                instance,
                svc_type,
                port,
                vec![
                    (String::from("silo"), alloc::format!("{}", silo_id)),
                ],
                silo_id,
            );
        }

        // Register on DHT as well
        self.discovery.register(ServiceType::Compute, "Qindows Node", 7700);
        self.discovery.register(ServiceType::Storage, "Prism Object Store", 7701);
        self.discovery.register(ServiceType::Relay, "Nexus Relay", 7703);

        self.booted = true;
    }

    /// Tick the discovery system — process events and bridge mDNS → DHT.
    ///
    /// Should be called periodically (e.g., every second) from the
    /// Nexus silo's event loop.
    pub fn tick(&mut self, now: u64) {
        self.stats.uptime_ticks = now;

        // Drain mDNS events and bridge to ServiceDiscovery
        let events = self.mdns.drain_events();
        for event in events {
            match event {
                MdnsEvent::ServiceFound(svc) => {
                    self.stats.mdns_services_found += 1;
                    // Convert mDNS DiscoveredService → DHT ServiceRecord
                    let record = mdns_to_service_record(&svc, now);
                    self.discovery.process_advertisement(record, 0, 1);
                }
                MdnsEvent::ServiceUpdated(svc) => {
                    let record = mdns_to_service_record(&svc, now);
                    self.discovery.process_advertisement(record, 0, 1);
                }
                MdnsEvent::ServiceLost(_name) => {
                    // Let the expiry mechanism handle it
                }
            }
        }

        // Expire stale services from both systems
        self.mdns.expire_services(now, 1_000_000); // 1M ticks/sec assumed
        self.discovery.expire(now);

        self.stats.active_services =
            self.discovery.remote_services.len() +
            self.mdns.list_discovered().len();
    }

    /// Browse for services — searches both mDNS (LAN) and DHT (WAN).
    pub fn browse(&mut self, service_type: ServiceType) -> Vec<BrowseResult> {
        let mut results = Vec::new();

        // Search DHT
        for svc in self.discovery.discover(service_type) {
            results.push(BrowseResult {
                name: svc.record.name.clone(),
                source: DiscoverySource::Dht,
                latency_ms: svc.latency_ms,
                port: svc.record.endpoints.first().map(|e| e.port).unwrap_or(0),
                addr: svc.record.endpoints.first().map(|e| e.addr).unwrap_or([0; 4]),
            });
        }

        // Also check mDNS local cache
        for svc in self.mdns.list_discovered() {
            results.push(BrowseResult {
                name: svc.name.clone(),
                source: DiscoverySource::Mdns,
                latency_ms: 0, // LAN
                port: svc.port,
                addr: svc.address,
            });
        }

        // Sort by latency (closest first)
        results.sort_by_key(|r| r.latency_ms);
        results
    }

    /// Find the single best service of a type (lowest latency, highest capacity).
    pub fn find_best(&mut self, service_type: ServiceType) -> Option<BrowseResult> {
        self.browse(service_type).into_iter().next()
    }

    /// Get combined stats.
    pub fn stats(&self) -> &MeshDiscoveryStats {
        &self.stats
    }

    /// Shutdown — unregister all services (sends mDNS goodbye).
    pub fn shutdown(&mut self) {
        for &(instance, _, _) in QINDOWS_SERVICES {
            self.mdns.unregister_service(instance);
        }
        self.mdns.unregister_service(&self.hostname);
    }
}

/// A unified browse result from either mDNS or DHT.
#[derive(Debug, Clone)]
pub struct BrowseResult {
    /// Service name
    pub name: String,
    /// Where this result came from
    pub source: DiscoverySource,
    /// Round-trip latency in milliseconds
    pub latency_ms: u32,
    /// Service port
    pub port: u16,
    /// IPv4 address
    pub addr: [u8; 4],
}

/// Where a discovery result originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    /// Found via mDNS (local network)
    Mdns,
    /// Found via DHT (global mesh)
    Dht,
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Convert an mDNS DiscoveredService into a DHT ServiceRecord.
fn mdns_to_service_record(svc: &MdnsDiscovered, now: u64) -> ServiceRecord {
    ServiceRecord {
        service_id: hash_service_name(&svc.name),
        node_id: [0u8; 32], // Unknown from mDNS alone
        service_type: ServiceType::Compute, // Default; refined by TXT records
        name: svc.name.clone(),
        version: 1,
        endpoints: vec![Endpoint {
            addr: svc.address,
            port: svc.port,
            direct: true,
        }],
        capacity: 1.0,
        last_seen: now,
        ttl: svc.ttl,
        metadata: svc.txt.clone(),
    }
}

/// Simple hash of a service name for deterministic IDs.
fn hash_service_name(name: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3); // FNV prime
    }
    h
}
