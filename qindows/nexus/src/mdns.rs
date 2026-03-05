//! # Nexus mDNS / Service Discovery
//!
//! RFC 6762 Multicast DNS for zero-configuration networking.
//! Allows Silos and mesh peers to discover services without
//! a central DNS server — just broadcast on 224.0.0.251:5353.
//!
//! Used by the mesh for automatic peer discovery, by Aether
//! for discovering printers/displays, and by Chimera for
//! network share browsing.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// mDNS multicast address (IPv4).
pub const MDNS_MULTICAST_V4: [u8; 4] = [224, 0, 0, 251];
/// mDNS port.
pub const MDNS_PORT: u16 = 5353;
/// Response TTL for local services (default: 2 minutes).
pub const DEFAULT_TTL: u32 = 120;

/// DNS resource record types used in mDNS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RrType {
    A     = 1,    // IPv4 address
    Ptr   = 12,   // Domain pointer
    Txt   = 16,   // Text records (key=value)
    Aaaa  = 28,   // IPv6 address
    Srv   = 33,   // Service locator
    Any   = 255,  // Wildcard query
}

impl RrType {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            1   => Some(RrType::A),
            12  => Some(RrType::Ptr),
            16  => Some(RrType::Txt),
            28  => Some(RrType::Aaaa),
            33  => Some(RrType::Srv),
            255 => Some(RrType::Any),
            _   => None,
        }
    }
}

/// A DNS resource record.
#[derive(Debug, Clone)]
pub struct ResourceRecord {
    /// Fully qualified name (e.g., "_http._tcp.local")
    pub name: String,
    /// Record type
    pub rr_type: RrType,
    /// Class (always IN = 1 for mDNS, with cache-flush bit)
    pub class: u16,
    /// Time to live (seconds)
    pub ttl: u32,
    /// Record data
    pub rdata: RData,
}

/// Resource record data variants.
#[derive(Debug, Clone)]
pub enum RData {
    /// A record: IPv4 address
    A([u8; 4]),
    /// AAAA record: IPv6 address
    Aaaa([u8; 16]),
    /// PTR record: domain name pointer
    Ptr(String),
    /// SRV record: service locator
    Srv { priority: u16, weight: u16, port: u16, target: String },
    /// TXT record: key-value pairs
    Txt(Vec<(String, String)>),
}

/// A locally registered service.
#[derive(Debug, Clone)]
pub struct LocalService {
    /// Instance name (e.g., "My Printer")
    pub instance: String,
    /// Service type (e.g., "_http._tcp")
    pub service_type: String,
    /// Domain (always "local")
    pub domain: String,
    /// Port number
    pub port: u16,
    /// Host (e.g., "qindows-pc.local")
    pub host: String,
    /// IPv4 address
    pub address: [u8; 4],
    /// TXT key-value metadata
    pub txt: Vec<(String, String)>,
    /// Is this service actively advertised?
    pub active: bool,
    /// Silo ID that owns this service
    pub silo_id: u64,
}

impl LocalService {
    /// Full service name (instance._type.domain.)
    pub fn full_name(&self) -> String {
        alloc::format!("{}.{}.{}.", self.instance, self.service_type, self.domain)
    }

    /// Generate the DNS records for this service.
    pub fn to_records(&self) -> Vec<ResourceRecord> {
        let full = self.full_name();
        let svc_name = alloc::format!("{}.{}.", self.service_type, self.domain);
        let host = alloc::format!("{}.", self.host);

        alloc::vec![
            // PTR: _http._tcp.local. → instance._http._tcp.local.
            ResourceRecord {
                name: svc_name,
                rr_type: RrType::Ptr,
                class: 1,
                ttl: DEFAULT_TTL,
                rdata: RData::Ptr(full.clone()),
            },
            // SRV: instance._http._tcp.local. → host:port
            ResourceRecord {
                name: full.clone(),
                rr_type: RrType::Srv,
                class: 1 | 0x8000, // Cache-flush
                ttl: DEFAULT_TTL,
                rdata: RData::Srv {
                    priority: 0,
                    weight: 0,
                    port: self.port,
                    target: host.clone(),
                },
            },
            // TXT: instance._http._tcp.local. → key=value pairs
            ResourceRecord {
                name: full,
                rr_type: RrType::Txt,
                class: 1 | 0x8000,
                ttl: DEFAULT_TTL,
                rdata: RData::Txt(self.txt.clone()),
            },
            // A: host.local. → IPv4
            ResourceRecord {
                name: host,
                rr_type: RrType::A,
                class: 1 | 0x8000,
                ttl: DEFAULT_TTL,
                rdata: RData::A(self.address),
            },
        ]
    }
}

/// A discovered remote service.
#[derive(Debug, Clone)]
pub struct DiscoveredService {
    /// Full service name
    pub name: String,
    /// Service type
    pub service_type: String,
    /// Host
    pub host: String,
    /// Port
    pub port: u16,
    /// IPv4 address
    pub address: [u8; 4],
    /// TXT metadata
    pub txt: Vec<(String, String)>,
    /// When this was last seen (ticks)
    pub last_seen: u64,
    /// TTL (seconds)
    pub ttl: u32,
}

/// mDNS event for the application layer.
#[derive(Debug, Clone)]
pub enum MdnsEvent {
    /// A new service was discovered
    ServiceFound(DiscoveredService),
    /// A service was updated (new TXT, address change)
    ServiceUpdated(DiscoveredService),
    /// A service has gone away (TTL expired or goodbye)
    ServiceLost(String),
}

/// mDNS query types.
#[derive(Debug, Clone)]
pub struct MdnsQuery {
    /// Name to query
    pub name: String,
    /// Record type
    pub rr_type: RrType,
    /// Is this a unicast-response query?
    pub unicast_response: bool,
}

/// mDNS statistics.
#[derive(Debug, Clone, Default)]
pub struct MdnsStats {
    pub queries_sent: u64,
    pub queries_received: u64,
    pub responses_sent: u64,
    pub responses_received: u64,
    pub services_registered: u64,
    pub services_discovered: u64,
    pub conflicts: u64,
}

/// The mDNS Responder.
pub struct MdnsResponder {
    /// Locally registered services
    pub services: Vec<LocalService>,
    /// Discovered remote services
    pub discovered: BTreeMap<String, DiscoveredService>,
    /// Pending events for the application
    pub events: Vec<MdnsEvent>,
    /// Our hostname
    pub hostname: String,
    /// Our IPv4 address
    pub address: [u8; 4],
    /// Stats
    pub stats: MdnsStats,
}

impl MdnsResponder {
    pub fn new(hostname: &str, address: [u8; 4]) -> Self {
        MdnsResponder {
            services: Vec::new(),
            discovered: BTreeMap::new(),
            events: Vec::new(),
            hostname: String::from(hostname),
            address,
            stats: MdnsStats::default(),
        }
    }

    /// Register a local service for advertisement.
    pub fn register_service(
        &mut self,
        instance: &str,
        service_type: &str,
        port: u16,
        txt: Vec<(String, String)>,
        silo_id: u64,
    ) {
        let svc = LocalService {
            instance: String::from(instance),
            service_type: String::from(service_type),
            domain: String::from("local"),
            port,
            host: alloc::format!("{}.local", self.hostname),
            address: self.address,
            txt,
            active: true,
            silo_id,
        };

        self.services.push(svc);
        self.stats.services_registered += 1;
    }

    /// Unregister a service (sends a goodbye with TTL=0).
    pub fn unregister_service(&mut self, instance: &str) -> bool {
        if let Some(svc) = self.services.iter_mut().find(|s| s.instance == instance) {
            svc.active = false;
            true
        } else {
            false
        }
    }

    /// Handle an incoming mDNS query — generate response records.
    pub fn handle_query(&mut self, query: &MdnsQuery) -> Vec<ResourceRecord> {
        self.stats.queries_received += 1;
        let mut answers = Vec::new();

        for svc in &self.services {
            if !svc.active { continue; }

            let svc_name = alloc::format!("{}.{}.", svc.service_type, svc.domain);
            let full_name = svc.full_name();
            let host_name = alloc::format!("{}.", svc.host);

            let matches = match query.rr_type {
                RrType::Ptr => query.name == svc_name,
                RrType::Srv | RrType::Txt => query.name == full_name,
                RrType::A => query.name == host_name,
                RrType::Any => {
                    query.name == svc_name
                        || query.name == full_name
                        || query.name == host_name
                }
                _ => false,
            };

            if matches {
                answers.extend(svc.to_records());
            }
        }

        if !answers.is_empty() {
            self.stats.responses_sent += 1;
        }

        answers
    }

    /// Process an incoming mDNS response — update discovered services.
    pub fn handle_response(&mut self, records: &[ResourceRecord], now: u64) {
        self.stats.responses_received += 1;

        // Extract SRV, A, TXT records and correlate them
        let mut srv_records: Vec<&ResourceRecord> = Vec::new();
        let mut a_records: Vec<&ResourceRecord> = Vec::new();
        let mut txt_records: Vec<&ResourceRecord> = Vec::new();

        for rr in records {
            match rr.rr_type {
                RrType::Srv => srv_records.push(rr),
                RrType::A   => a_records.push(rr),
                RrType::Txt => txt_records.push(rr),
                _ => {}
            }
        }

        for srv in &srv_records {
            if let RData::Srv { port, target, .. } = &srv.rdata {
                    // Goodbye (TTL=0) — remove service
                if srv.ttl == 0 {
                    if self.discovered.remove(&srv.name).is_some() {
                        self.events.push(MdnsEvent::ServiceLost(srv.name.clone()));
                    }
                    continue;
                }

                // Find matching A record
                let address = a_records.iter()
                    .find(|a| a.name == *target)
                    .and_then(|a| if let RData::A(ip) = &a.rdata { Some(*ip) } else { None })
                    .unwrap_or([0, 0, 0, 0]);

                // Find matching TXT record
                let txt = txt_records.iter()
                    .find(|t| t.name == srv.name)
                    .and_then(|t| if let RData::Txt(pairs) = &t.rdata { Some(pairs.clone()) } else { None })
                    .unwrap_or_default();

                // Extract service type from service name
                let service_type = srv.name.splitn(2, '.').nth(1)
                    .unwrap_or(&srv.name).trim_end_matches('.').to_string();

                let discovered = DiscoveredService {
                    name: srv.name.clone(),
                    service_type,
                    host: target.clone(),
                    port: *port,
                    address,
                    txt,
                    last_seen: now,
                    ttl: srv.ttl,
                };

                let is_new = !self.discovered.contains_key(&srv.name);
                self.discovered.insert(srv.name.clone(), discovered.clone());

                if is_new {
                    self.stats.services_discovered += 1;
                    self.events.push(MdnsEvent::ServiceFound(discovered));
                } else {
                    self.events.push(MdnsEvent::ServiceUpdated(discovered));
                }
            }
        }
    }

    /// Expire discovered services whose TTL has elapsed.
    pub fn expire_services(&mut self, now: u64, ticks_per_second: u64) {
        let expired: Vec<String> = self.discovered.iter()
            .filter(|(_, svc)| {
                let age_secs = (now.saturating_sub(svc.last_seen)) / ticks_per_second;
                age_secs > svc.ttl as u64
            })
            .map(|(name, _)| name.clone())
            .collect();

        for name in expired {
            self.discovered.remove(&name);
            self.events.push(MdnsEvent::ServiceLost(name));
        }
    }

    /// Drain pending events.
    pub fn drain_events(&mut self) -> Vec<MdnsEvent> {
        core::mem::take(&mut self.events)
    }

    /// Browse for services of a given type (generates a PTR query).
    pub fn browse(&mut self, service_type: &str) -> MdnsQuery {
        self.stats.queries_sent += 1;
        MdnsQuery {
            name: alloc::format!("{}.local.", service_type),
            rr_type: RrType::Ptr,
            unicast_response: false,
        }
    }

    /// List all discovered services.
    pub fn list_discovered(&self) -> Vec<&DiscoveredService> {
        self.discovered.values().collect()
    }
}
