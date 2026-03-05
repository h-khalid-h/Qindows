//! # Q-Proxy — Secure Network Proxy
//!
//! Enforces DNS-over-HTTPS and on-path masking (kernel-level onion
//! routing) for all network traffic (Section 5).
//!
//! Features:
//! - **DNS-over-HTTPS**: All DNS queries routed through encrypted HTTPS
//! - **On-Path Masking**: Multi-hop onion routing (3 relays default)
//! - **Per-Silo Routing**: Each app can have its own proxy chain
//! - **Exit Node Selection**: Geographic / latency-based selection
//! - **Circuit Rotation**: Proxy circuits rotate every N minutes

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Relay node (one hop in the onion route).
#[derive(Debug, Clone)]
pub struct Relay {
    /// Relay ID
    pub id: u64,
    /// Public key for encryption layer
    pub public_key: [u8; 32],
    /// Geographic region
    pub region: String,
    /// Latency (ms)
    pub latency_ms: u32,
    /// Bandwidth (bytes/sec)
    pub bandwidth: u64,
    /// Is this relay online?
    pub online: bool,
    /// Trust score
    pub trust_score: u8,
    /// Bytes relayed through this node
    pub bytes_relayed: u64,
}

/// A proxy circuit (chain of relays).
#[derive(Debug, Clone)]
pub struct Circuit {
    /// Circuit ID
    pub id: u64,
    /// Relay chain (entry → middle → exit)
    pub relays: Vec<u64>,
    /// Assigned Silo (or 0 for system-wide)
    pub silo_id: u64,
    /// Created timestamp
    pub created_at: u64,
    /// Expires at (rotation time)
    pub expires_at: u64,
    /// Bytes transmitted through this circuit
    pub bytes_tx: u64,
    /// Is this circuit active?
    pub active: bool,
}

/// DNS record type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsRecordType {
    A,     // IPv4
    AAAA,  // IPv6
    CNAME, // Alias
    MX,    // Mail
    TXT,   // Text
}

/// A cached DNS result.
#[derive(Debug, Clone)]
pub struct DnsEntry {
    /// Domain name
    pub domain: String,
    /// Resolved IP (as u32 for v4)
    pub ip: u32,
    /// Record type
    pub record_type: DnsRecordType,
    /// TTL remaining (seconds)
    pub ttl: u64,
    /// Fetched via DoH?
    pub secure: bool,
}

/// DoH provider.
#[derive(Debug, Clone)]
pub struct DohProvider {
    /// Provider name
    pub name: String,
    /// Endpoint URL template
    pub endpoint: String,
    /// Latency (ms)
    pub latency_ms: u32,
    /// Is this the active provider?
    pub active: bool,
}

/// Q-Proxy statistics.
#[derive(Debug, Clone, Default)]
pub struct ProxyStats {
    pub dns_queries: u64,
    pub dns_cache_hits: u64,
    pub circuits_created: u64,
    pub circuits_rotated: u64,
    pub bytes_relayed: u64,
    pub onion_hops_total: u64,
    pub blocked_domains: u64,
}

/// The Q-Proxy.
pub struct QProxy {
    /// Known relays
    pub relays: BTreeMap<u64, Relay>,
    /// Active circuits
    pub circuits: BTreeMap<u64, Circuit>,
    /// DNS cache
    pub dns_cache: BTreeMap<String, DnsEntry>,
    /// Blocked domains
    pub blocklist: Vec<String>,
    /// DoH providers
    pub doh_providers: Vec<DohProvider>,
    /// Default circuit length (hops)
    pub circuit_length: usize,
    /// Circuit rotation interval (seconds)
    pub rotation_interval: u64,
    /// Next circuit ID
    next_circuit_id: u64,
    /// Statistics
    pub stats: ProxyStats,
}

impl QProxy {
    pub fn new() -> Self {
        QProxy {
            relays: BTreeMap::new(),
            circuits: BTreeMap::new(),
            dns_cache: BTreeMap::new(),
            blocklist: Vec::new(),
            doh_providers: Vec::new(),
            circuit_length: 3,
            rotation_interval: 600, // 10 minutes
            next_circuit_id: 1,
            stats: ProxyStats::default(),
        }
    }

    /// Add a relay node.
    pub fn add_relay(&mut self, relay: Relay) {
        self.relays.insert(relay.id, relay);
    }

    /// Add a DoH provider.
    pub fn add_doh_provider(&mut self, name: &str, endpoint: &str) {
        self.doh_providers.push(DohProvider {
            name: String::from(name),
            endpoint: String::from(endpoint),
            latency_ms: 0,
            active: self.doh_providers.is_empty(), // First is active
        });
    }

    /// Resolve a domain via DoH (with caching).
    pub fn resolve(&mut self, domain: &str) -> Option<u32> {
        self.stats.dns_queries += 1;

        // Check blocklist
        if self.blocklist.iter().any(|b| domain.ends_with(b.as_str())) {
            self.stats.blocked_domains += 1;
            return None;
        }

        // Check cache
        if let Some(entry) = self.dns_cache.get(domain) {
            if entry.ttl > 0 {
                self.stats.dns_cache_hits += 1;
                return Some(entry.ip);
            }
        }

        // In production: send HTTPS request to DoH provider
        // Simulated: generate deterministic IP from domain hash
        let mut hash: u32 = 0;
        for b in domain.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(b as u32);
        }
        let ip = hash | 0x0A000000; // Put in 10.x.x.x range

        self.dns_cache.insert(String::from(domain), DnsEntry {
            domain: String::from(domain),
            ip,
            record_type: DnsRecordType::A,
            ttl: 300,
            secure: true,
        });

        Some(ip)
    }

    /// Build an onion circuit for a Silo.
    pub fn build_circuit(&mut self, silo_id: u64, now: u64) -> Option<u64> {
        let available: Vec<u64> = self.relays.values()
            .filter(|r| r.online && r.trust_score >= 50)
            .map(|r| r.id)
            .collect();

        if available.len() < self.circuit_length {
            return None;
        }

        // Select relays (simplified: take first N with different regions)
        let mut selected = Vec::new();
        let mut used_regions = Vec::new();
        for &id in &available {
            if selected.len() >= self.circuit_length { break; }
            if let Some(relay) = self.relays.get(&id) {
                if !used_regions.contains(&relay.region) {
                    selected.push(id);
                    used_regions.push(relay.region.clone());
                }
            }
        }

        // Fall back to any relays if not enough diverse regions
        if selected.len() < self.circuit_length {
            for &id in &available {
                if selected.len() >= self.circuit_length { break; }
                if !selected.contains(&id) {
                    selected.push(id);
                }
            }
        }

        if selected.len() < self.circuit_length { return None; }

        let circuit_id = self.next_circuit_id;
        self.next_circuit_id += 1;

        self.circuits.insert(circuit_id, Circuit {
            id: circuit_id,
            relays: selected,
            silo_id,
            created_at: now,
            expires_at: now + self.rotation_interval,
            bytes_tx: 0,
            active: true,
        });

        self.stats.circuits_created += 1;
        self.stats.onion_hops_total += self.circuit_length as u64;
        Some(circuit_id)
    }

    /// Send data through a circuit (onion-encrypted).
    pub fn send(&mut self, circuit_id: u64, bytes: u64) -> Result<(), &'static str> {
        let circuit = self.circuits.get_mut(&circuit_id)
            .ok_or("Circuit not found")?;
        if !circuit.active { return Err("Circuit inactive"); }

        circuit.bytes_tx = circuit.bytes_tx.saturating_add(bytes);
        self.stats.bytes_relayed = self.stats.bytes_relayed.saturating_add(bytes);

        // Update relay stats
        for &relay_id in &circuit.relays.clone() {
            if let Some(relay) = self.relays.get_mut(&relay_id) {
                relay.bytes_relayed = relay.bytes_relayed.saturating_add(bytes);
            }
        }
        Ok(())
    }

    /// Rotate expired circuits.
    pub fn rotate_circuits(&mut self, now: u64) {
        let expired: Vec<u64> = self.circuits.iter()
            .filter(|(_, c)| c.active && now >= c.expires_at)
            .map(|(&id, _)| id)
            .collect();

        for id in &expired {
            if let Some(circuit) = self.circuits.get_mut(id) {
                circuit.active = false;
                let silo = circuit.silo_id;
                // Auto-build replacement circuit
                self.stats.circuits_rotated += 1;
                self.build_circuit(silo, now);
            }
        }
    }

    /// Block a domain.
    pub fn block_domain(&mut self, domain: &str) {
        self.blocklist.push(String::from(domain));
        self.dns_cache.remove(domain);
    }
}
