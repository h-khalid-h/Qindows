//! # Universal Namespace (UNS) — Phase 58
//!
//! The Universal Namespace makes EVERY resource in Qindows addressable
//! through a single, unified URI scheme regardless of location:
//!
//! ```text
//! prism://                      — versioned object store
//! qfa://node-id/silo/service    — Q-Fabric network endpoint
//! dev://gpu0, dev://nvme0       — hardware device objects
//! env://hostname, env://ip      — environment variables
//! cap://silo-id/capability-name — capability token descriptor
//! ```
//!
//! ## Q-Manifest Law 9: Universal Namespace
//! The UNS is the ONLY way Silos discover resources. There are no:
//! - Drive letters (no `C:\`, no `/dev/sda`)
//! - Process IDs as raw integers (no `kill(1234)` — use `qfa://node/silo`)
//! - Direct file paths (`/etc/passwd` is `prism://system/passwd`)
//!
//! ## Architecture Guardian
//! The UNS is a **kernel service Silo** — not a kernel data structure.
//! This module defines the KERNEL INTERFACE: the resolver that maps
//! URIs to their backing objects before returning to the requesting Silo.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── UNS URI Scheme Registry ───────────────────────────────────────────────────

/// A parsed UNS URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsUri {
    /// Scheme: "prism", "qfa", "dev", "env", "cap"
    pub scheme: String,
    /// Authority (node ID, device name, etc.)
    pub authority: String,
    /// Path segments
    pub path: Vec<String>,
    /// Original raw string
    pub raw: String,
}

impl UnsUri {
    /// Parse a UNS URI string.
    ///
    /// Format: `scheme://authority/path/segments`
    ///
    /// Returns `None` if the URI is malformed or the scheme is unrecognized.
    pub fn parse(uri: &str) -> Option<Self> {
        let (scheme, rest) = uri.split_once("://")?;
        let scheme = scheme.to_lowercase();

        // Validate scheme
        match scheme.as_str() {
            "prism" | "qfa" | "dev" | "env" | "cap" => {}
            _ => return None,
        }

        let (authority, path_str) = rest.split_once('/')
            .unwrap_or((rest, ""));

        let path: Vec<String> = path_str
            .split('/')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        Some(UnsUri {
            scheme: String::from(scheme),
            authority: String::from(authority),
            path,
            raw: String::from(uri),
        })
    }

    pub fn is_local(&self) -> bool {
        self.authority.is_empty() || self.authority == "local" || self.authority == "."
    }
}

// ── UNS Resolution Result ─────────────────────────────────────────────────────

/// What a resolved URI points to.
#[derive(Debug, Clone)]
pub enum UnsTarget {
    /// A Prism versioned object
    PrismObject { oid: u64 },
    /// A Q-Fabric network endpoint
    QFabricEndpoint { node_id: [u8; 16], silo_id: u64, service: String },
    /// A hardware device object
    DeviceObject { device_id: u32, device_name: String },
    /// An environment variable value
    EnvValue { key: String, value: String },
    /// A capability token descriptor
    CapabilityToken { silo_id: u64, cap_name: String },
    /// A remote URI (must be forwarded to the remote node's UNS)
    Remote { node_id: String },
}

/// Result of a UNS resolution.
#[derive(Debug, Clone)]
pub struct UnsResolution {
    pub uri: UnsUri,
    pub target: UnsTarget,
    pub resolved_by: &'static str,
    /// Whether the result was served from cache
    pub from_cache: bool,
}

// ── UNS Mount Points ──────────────────────────────────────────────────────────

/// A mount endpoint: maps a UNS prefix to a resolver.
#[derive(Debug, Clone)]
pub struct UnsMountPoint {
    /// The URI prefix this mount handles (e.g., "prism://")
    pub prefix: String,
    /// Name of the Silo that owns this mount
    pub owner_silo: u64,
    /// Whether the mount is read-only
    pub read_only: bool,
    /// Total resolutions served by this mount
    pub resolutions: u64,
}

// ── UNS Cache ─────────────────────────────────────────────────────────────────

/// A cached UNS resolution entry with TTL.
#[derive(Debug, Clone)]
pub struct UnsCacheEntry {
    pub resolution: UnsResolution,
    /// Kernel tick when this entry expires
    pub expires_at: u64,
}

// ── Universal Namespace Resolver ──────────────────────────────────────────────

/// Statistics for the UNS resolver.
#[derive(Debug, Default, Clone)]
pub struct UnsStats {
    pub total_queries: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub resolution_errors: u64,
    pub mount_points_registered: u64,
}

/// The Universal Namespace Resolver.
///
/// Accepts raw URI strings, parses them, resolves scheme-specific targets,
/// and caches results with TTLs. Called by the `PrismOpen` syscall handler
/// and any other syscall that accepts a URI as an argument.
pub struct UniversalNamespace {
    /// Registered scheme handlers (scheme → mount points)
    pub mounts: BTreeMap<String, Vec<UnsMountPoint>>,
    /// Resolution cache (raw URI → cached result)
    pub cache: BTreeMap<String, UnsCacheEntry>,
    /// Environment variable store (replaces shell env for Silos)
    pub env_store: BTreeMap<String, String>,
    /// Device object registry (device name → device_id)
    pub device_registry: BTreeMap<String, u32>,
    /// Cache TTL in kernel ticks (default: 1000 = ~1 second)
    pub cache_ttl: u64,
    /// Statistics
    pub stats: UnsStats,
}

impl UniversalNamespace {
    pub fn new() -> Self {
        let mut uns = UniversalNamespace {
            mounts: BTreeMap::new(),
            cache: BTreeMap::new(),
            env_store: BTreeMap::new(),
            device_registry: BTreeMap::new(),
            cache_ttl: 1000,
            stats: UnsStats::default(),
        };
        // Pre-register core environment variables
        uns.env_store.insert(String::from("OS"), String::from("Qindows"));
        uns.env_store.insert(String::from("VERSION"), String::from("0.1.0-dev"));
        uns
    }

    /// Register a device in the UNS device registry.
    pub fn register_device(&mut self, name: &str, device_id: u32) {
        self.device_registry.insert(String::from(name), device_id);
        crate::serial_println!("[UNS] Device registered: dev://{} → id {}", name, device_id);
    }

    /// Register a UNS mount point for a scheme.
    pub fn register_mount(&mut self, prefix: &str, owner_silo: u64, read_only: bool) {
        let scheme = prefix.split("://").next().unwrap_or(prefix).to_lowercase();
        let mount = UnsMountPoint {
            prefix: String::from(prefix),
            owner_silo,
            read_only,
            resolutions: 0,
        };
        self.mounts.entry(scheme).or_default().push(mount);
        self.stats.mount_points_registered += 1;
        crate::serial_println!("[UNS] Mount registered: {} (silo {})", prefix, owner_silo);
    }

    /// Set an environment variable.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env_store.insert(String::from(key), String::from(value));
    }

    /// Resolve a URI string to its target.
    ///
    /// This is the primary UNS query API. Checks cache first, then
    /// dispatches to scheme-specific resolvers.
    pub fn resolve(&mut self, uri_str: &str, current_tick: u64) -> Result<UnsResolution, &'static str> {
        self.stats.total_queries += 1;

        // Cache check
        if let Some(cached) = self.cache.get(uri_str) {
            if cached.expires_at > current_tick {
                self.stats.cache_hits += 1;
                crate::serial_println!("[UNS] Cache hit: {}", uri_str);
                return Ok(cached.resolution.clone());
            }
        }
        self.stats.cache_misses += 1;

        // Parse URI
        let uri = UnsUri::parse(uri_str)
            .ok_or("UNS: malformed URI")?;

        // Dispatch to scheme resolver
        let target = match uri.scheme.as_str() {
            "prism" => self.resolve_prism(&uri),
            "qfa"   => self.resolve_qfa(&uri),
            "dev"   => self.resolve_dev(&uri),
            "env"   => self.resolve_env(&uri),
            "cap"   => self.resolve_cap(&uri),
            _       => Err("UNS: unknown scheme"),
        }?;

        let resolution = UnsResolution {
            target,
            resolved_by: "uns::resolve",
            from_cache: false,
            uri: uri.clone(),
        };

        // Cache the result
        self.cache.insert(String::from(uri_str), UnsCacheEntry {
            resolution: resolution.clone(),
            expires_at: current_tick + self.cache_ttl,
        });

        crate::serial_println!("[UNS] Resolved: {}", uri_str);
        Ok(resolution)
    }

    /// Invalidate all cache entries for a given URI prefix.
    ///
    /// Called when a Prism object is deleted or a device is hot-unplugged.
    pub fn invalidate_prefix(&mut self, prefix: &str) {
        let keys: Vec<String> = self.cache.keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        for k in keys {
            self.cache.remove(&k);
        }
        crate::serial_println!("[UNS] Cache invalidated for prefix: {}", prefix);
    }

    // ── Scheme resolvers ───────────────────────────────────────────────────

    fn resolve_prism(&self, uri: &UnsUri) -> Result<UnsTarget, &'static str> {
        // Production: look up Prism OID by path
        let oid = hash_path(&uri.path);
        Ok(UnsTarget::PrismObject { oid })
    }

    fn resolve_qfa(&self, uri: &UnsUri) -> Result<UnsTarget, &'static str> {
        // Format: qfa://node-id/silo-id/service-name
        let node_str = &uri.authority;
        let silo_id = uri.path.get(0)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let service = uri.path.get(1).cloned().unwrap_or_default();

        Ok(UnsTarget::QFabricEndpoint {
            node_id: parse_node_id(node_str),
            silo_id,
            service,
        })
    }

    fn resolve_dev(&self, uri: &UnsUri) -> Result<UnsTarget, &'static str> {
        let name = &uri.authority;
        let device_id = self.device_registry.get(name.as_str())
            .copied()
            .unwrap_or(u32::MAX);
        Ok(UnsTarget::DeviceObject {
            device_id,
            device_name: name.clone(),
        })
    }

    fn resolve_env(&self, uri: &UnsUri) -> Result<UnsTarget, &'static str> {
        let key = &uri.authority;
        let value = self.env_store.get(key.as_str())
            .cloned()
            .unwrap_or_default();
        Ok(UnsTarget::EnvValue { key: key.clone(), value })
    }

    fn resolve_cap(&self, uri: &UnsUri) -> Result<UnsTarget, &'static str> {
        // Format: cap://silo-id/capability-name
        let silo_id = uri.authority.parse::<u64>().unwrap_or(0);
        let cap_name = uri.path.get(0).cloned().unwrap_or_default();
        Ok(UnsTarget::CapabilityToken { silo_id, cap_name })
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn hash_path(segments: &[String]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for seg in segments {
        for b in seg.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h & 0x7FFF_FFFF_FFFF_FFFF
}

fn parse_node_id(s: &str) -> [u8; 16] {
    let mut id = [0u8; 16];
    let bytes = s.as_bytes();
    let len = bytes.len().min(16);
    id[..len].copy_from_slice(&bytes[..len]);
    id
}
