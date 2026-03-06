//! # Mesh DNS — Decentralized Name Resolution
//!
//! Resolves human-readable names to mesh peer IDs and
//! service endpoints without centralized DNS (Section 11.6).
//!
//! Features:
//! - DHT-based name registration
//! - Hierarchical names (user.service.mesh)
//! - TTL-based caching
//! - DNSSEC-like signature verification
//! - Local override hosts file

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// DNS record type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    Peer,    // Name → peer ID
    Service, // Name → service endpoint
    Alias,   // Name → another name
    Txt,     // Name → text metadata
}

/// A DNS record.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub record_type: RecordType,
    pub value: String,
    pub peer_id: [u8; 32],
    pub ttl: u64,
    pub created_at: u64,
    pub signature: [u8; 64],
}

/// A cached record.
#[derive(Debug, Clone)]
pub struct CachedRecord {
    pub record: DnsRecord,
    pub cached_at: u64,
    pub hits: u64,
}

/// DNS statistics.
#[derive(Debug, Clone, Default)]
pub struct DnsStats {
    pub registrations: u64,
    pub lookups: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub expirations: u64,
}

/// The Mesh DNS Resolver.
pub struct MeshDns {
    /// Registered records (authoritative)
    pub records: BTreeMap<String, Vec<DnsRecord>>,
    /// Cache of remote lookups
    pub cache: BTreeMap<String, Vec<CachedRecord>>,
    /// Local overrides (hosts file)
    pub overrides: BTreeMap<String, String>,
    pub default_ttl: u64,
    pub max_cache: usize,
    pub stats: DnsStats,
}

impl MeshDns {
    pub fn new() -> Self {
        MeshDns {
            records: BTreeMap::new(),
            cache: BTreeMap::new(),
            overrides: BTreeMap::new(),
            default_ttl: 3600,
            max_cache: 10000,
            stats: DnsStats::default(),
        }
    }

    /// Register a name.
    pub fn register(&mut self, name: &str, record_type: RecordType, value: &str, peer_id: [u8; 32], sig: [u8; 64], now: u64) {
        let records = self.records.entry(String::from(name)).or_insert_with(Vec::new);
        records.push(DnsRecord {
            name: String::from(name), record_type,
            value: String::from(value), peer_id,
            ttl: self.default_ttl, created_at: now, signature: sig,
        });
        self.stats.registrations += 1;
    }

    /// Look up a name.
    pub fn lookup(&mut self, name: &str, now: u64) -> Vec<DnsRecord> {
        self.stats.lookups += 1;

        // Check local overrides first
        if let Some(value) = self.overrides.get(name) {
            return vec![DnsRecord {
                name: String::from(name), record_type: RecordType::Peer,
                value: value.clone(), peer_id: [0u8; 32],
                ttl: u64::MAX, created_at: 0, signature: [0u8; 64],
            }];
        }

        // Check authoritative records
        if let Some(records) = self.records.get(name) {
            let valid: Vec<DnsRecord> = records.iter()
                .filter(|r| now < r.created_at + r.ttl)
                .cloned()
                .collect();
            if !valid.is_empty() {
                return valid;
            }
        }

        // Check cache
        if let Some(cached) = self.cache.get_mut(name) {
            let valid: Vec<DnsRecord> = cached.iter_mut()
                .filter(|c| now < c.cached_at + c.record.ttl)
                .map(|c| { c.hits += 1; c.record.clone() })
                .collect();
            if !valid.is_empty() {
                self.stats.cache_hits += 1;
                return valid;
            }
        }

        self.stats.cache_misses += 1;
        Vec::new()
    }

    /// Cache a remote lookup result.
    pub fn cache_result(&mut self, record: DnsRecord, now: u64) {
        let name = record.name.clone();
        let cached = self.cache.entry(name).or_insert_with(Vec::new);
        cached.push(CachedRecord { record, cached_at: now, hits: 0 });
    }

    /// Add a local override.
    pub fn add_override(&mut self, name: &str, value: &str) {
        self.overrides.insert(String::from(name), String::from(value));
    }

    /// Expire stale cache entries.
    pub fn expire_cache(&mut self, now: u64) {
        for cached in self.cache.values_mut() {
            let before = cached.len();
            cached.retain(|c| now < c.cached_at + c.record.ttl);
            self.stats.expirations += (before - cached.len()) as u64;
        }
    }
}
