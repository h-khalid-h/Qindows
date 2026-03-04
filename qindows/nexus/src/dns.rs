//! # Nexus DNS Resolver
//!
//! Stub DNS resolver for the Nexus networking stack.
//! Supports A, AAAA, CNAME, MX, and TXT record types
//! with a local cache and configurable upstream servers.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// DNS record types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecordType {
    A,      // IPv4
    Aaaa,   // IPv6
    Cname,  // Canonical name
    Mx,     // Mail exchange
    Txt,    // Text record
    Ns,     // Name server
    Soa,    // Start of authority
    Ptr,    // Reverse lookup
    Srv,    // Service locator
}

/// A DNS record value.
#[derive(Debug, Clone)]
pub enum RecordData {
    /// IPv4 address
    A([u8; 4]),
    /// IPv6 address
    Aaaa([u8; 16]),
    /// Canonical name
    Cname(String),
    /// Mail exchange (priority, host)
    Mx(u16, String),
    /// Text record
    Txt(String),
    /// Name server
    Ns(String),
    /// Service (priority, weight, port, target)
    Srv(u16, u16, u16, String),
}

/// A cached DNS record.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    /// Domain name
    pub name: String,
    /// Record type
    pub rtype: RecordType,
    /// Record data
    pub data: RecordData,
    /// Time-to-live (seconds)
    pub ttl: u32,
    /// Cache insertion time (ns)
    pub cached_at: u64,
}

impl DnsRecord {
    /// Is this record expired?
    pub fn is_expired(&self, now_ns: u64) -> bool {
        let age_s = (now_ns.saturating_sub(self.cached_at)) / 1_000_000_000;
        age_s > self.ttl as u64
    }
}

/// DNS query result.
#[derive(Debug, Clone)]
pub enum DnsResult {
    /// Successful resolution
    Ok(Vec<DnsRecord>),
    /// Name not found (NXDOMAIN)
    NotFound,
    /// Server error
    ServerError,
    /// Timeout
    Timeout,
}

/// A DNS upstream server.
#[derive(Debug, Clone)]
pub struct DnsServer {
    /// Server address (IPv4)
    pub address: [u8; 4],
    /// Port (usually 53)
    pub port: u16,
    /// Number of queries sent
    pub queries: u64,
    /// Number of failures
    pub failures: u64,
    /// Average response time (ms)
    pub avg_response_ms: u32,
}

/// The DNS Resolver.
pub struct DnsResolver {
    /// Cache: domain → records
    pub cache: BTreeMap<String, Vec<DnsRecord>>,
    /// Upstream DNS servers
    pub servers: Vec<DnsServer>,
    /// Hosts file overrides
    pub hosts: BTreeMap<String, [u8; 4]>,
    /// Stats
    pub stats: DnsStats,
    /// Max cache entries
    pub max_cache: usize,
}

/// DNS resolver statistics.
#[derive(Debug, Clone, Default)]
pub struct DnsStats {
    pub queries: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub upstream_queries: u64,
    pub failures: u64,
    pub nxdomain: u64,
}

impl DnsResolver {
    pub fn new() -> Self {
        let mut resolver = DnsResolver {
            cache: BTreeMap::new(),
            servers: Vec::new(),
            hosts: BTreeMap::new(),
            stats: DnsStats::default(),
            max_cache: 1000,
        };

        // Default DNS servers (Cloudflare + Google)
        resolver.servers.push(DnsServer {
            address: [1, 1, 1, 1], port: 53,
            queries: 0, failures: 0, avg_response_ms: 0,
        });
        resolver.servers.push(DnsServer {
            address: [8, 8, 8, 8], port: 53,
            queries: 0, failures: 0, avg_response_ms: 0,
        });

        // Localhost
        resolver.hosts.insert(String::from("localhost"), [127, 0, 0, 1]);

        resolver
    }

    /// Resolve a domain name to IPv4 addresses.
    pub fn resolve_a(&mut self, domain: &str, now_ns: u64) -> DnsResult {
        self.stats.queries += 1;

        // Check hosts file first
        if let Some(&addr) = self.hosts.get(domain) {
            return DnsResult::Ok(alloc::vec![DnsRecord {
                name: String::from(domain),
                rtype: RecordType::A,
                data: RecordData::A(addr),
                ttl: u32::MAX,
                cached_at: now_ns,
            }]);
        }

        // Check cache
        if let Some(records) = self.cache.get(domain) {
            let valid: Vec<DnsRecord> = records.iter()
                .filter(|r| !r.is_expired(now_ns) && r.rtype == RecordType::A)
                .cloned()
                .collect();
            if !valid.is_empty() {
                self.stats.cache_hits += 1;
                return DnsResult::Ok(valid);
            }
        }

        self.stats.cache_misses += 1;
        // In production: send UDP query to upstream server
        self.stats.upstream_queries += 1;
        DnsResult::NotFound
    }

    /// Resolve any record type.
    pub fn resolve(&mut self, domain: &str, rtype: RecordType, now_ns: u64) -> DnsResult {
        self.stats.queries += 1;

        // Check cache
        if let Some(records) = self.cache.get(domain) {
            let valid: Vec<DnsRecord> = records.iter()
                .filter(|r| !r.is_expired(now_ns) && r.rtype == rtype)
                .cloned()
                .collect();
            if !valid.is_empty() {
                self.stats.cache_hits += 1;
                return DnsResult::Ok(valid);
            }
        }

        self.stats.cache_misses += 1;
        self.stats.upstream_queries += 1;
        DnsResult::NotFound
    }

    /// Insert a record into the cache.
    pub fn cache_record(&mut self, record: DnsRecord) {
        let name = record.name.clone();
        self.cache.entry(name).or_insert_with(Vec::new).push(record);

        // Evict if over limit
        while self.cache.len() > self.max_cache {
            if let Some(first_key) = self.cache.keys().next().cloned() {
                self.cache.remove(&first_key);
            }
        }
    }

    /// Flush expired entries from the cache.
    pub fn flush_expired(&mut self, now_ns: u64) {
        for records in self.cache.values_mut() {
            records.retain(|r| !r.is_expired(now_ns));
        }
        self.cache.retain(|_, records| !records.is_empty());
    }

    /// Add a hosts file entry.
    pub fn add_host(&mut self, domain: &str, addr: [u8; 4]) {
        self.hosts.insert(String::from(domain), addr);
    }

    /// Add an upstream DNS server.
    pub fn add_server(&mut self, addr: [u8; 4], port: u16) {
        self.servers.push(DnsServer {
            address: addr, port,
            queries: 0, failures: 0, avg_response_ms: 0,
        });
    }

    /// Flush entire cache.
    pub fn flush_cache(&mut self) {
        self.cache.clear();
    }
}
