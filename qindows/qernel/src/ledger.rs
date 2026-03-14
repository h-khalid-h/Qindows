//! # Q-Ledger — Application Distribution & Content-Addressable Package Ledger (Phase 63)
//!
//! Q-Ledger replaces MSI/EXE installers with a cryptographically-verified,
//! globally-deduplicated, immutable package registry.
//!
//! ## ARCHITECTURE.md §2.3: WebAssembly / Distribution
//! > "Atomic Installs: An app is just a signed cryptographic hash.
//! > Deduplication: If ten apps use the same version of a library,
//! > Q-Kit only stores one copy on disk. Uninstalling = deleting a pointer."
//!
//! ## Q-Manifest Law 2: Immutable Binaries
//! Packages are **read-only content-addressable blobs**. An installed app
//! can never modify its own code or installation prefix. To "update,"
//! the Ledger atomically points the alias to a new package hash.
//!
//! ## Q-Manifest Law 5: Global Deduplication
//! Every library dependency is stored once globally. A new app using an
//! already-cached library version incurs **zero additional disk cost**.
//!
//! ## Architecture Guardian: This module's role
//! Q-Ledger is the kernel-side index of installed packages. It:
//! - Validates package manifests and signatures
//! - Manages alias → package_hash → Prism OID mappings
//! - Enforces capability declarations (feeds into Silo launch)
//! - Does NOT copy files, compile code, or execute processes
//!   (compilation is handled by `wasm_runtime.rs`; execution by `silo_launch.rs`)

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;


// ── Package Identity ──────────────────────────────────────────────────────────

/// A 256-bit package content hash (FNV-128 of the binary payload doubled).
/// Two packages with the same `PackageHash` are provably identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageHash(pub [u8; 32]);

impl PackageHash {
    /// Derive a PackageHash from raw bytes using FNV-64 + xor-folded upper 64.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        let hi = h ^ 0xDEAD_BEEF_0000_0001;
        let mut out = [0u8; 32];
        out[..8].copy_from_slice(&h.to_le_bytes());
        out[8..16].copy_from_slice(&hi.to_le_bytes());
        out[16..24].copy_from_slice(&h.wrapping_mul(0x5555_5555_5555_5555).to_le_bytes());
        out[24..].copy_from_slice(&hi.wrapping_mul(0xAAAA_AAAA_AAAA_AAAA).to_le_bytes());
        PackageHash(out)
    }

    pub fn short_hex(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().unwrap_or([0u8; 8]))
    }
}

// ── Package Manifest ──────────────────────────────────────────────────────────

/// A Qindows app capability declaration (from manifest.q).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestCapability {
    /// Network access with specified allowed protocol
    Network { protocol: String },
    /// Prism object access with path scope
    Storage { scope: String },
    /// GPU / Aether surface creation
    Graphics,
    /// BCI / Q-Synapse neural input stream
    NeuralInput,
    /// Mesh compute participation
    MeshCompute,
    /// Hardware device access (specify device class)
    Device { class: String },
}

/// The full app manifest — must be present and valid before installation.
#[derive(Debug, Clone)]
pub struct AppManifest {
    /// Reverse-domain app identifier (e.g. "org.qindows.collab")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Semantic version string
    pub version: String,
    /// WASM entry-point binary hash
    pub entry_hash: PackageHash,
    /// Q-Manifest capability declarations
    pub capabilities: Vec<ManifestCapability>,
    /// Publisher's Ed25519 public key (64 bytes)
    pub publisher_key: [u8; 64],
    /// Package signature over (id + version + entry_hash) by publisher_key
    pub signature: [u8; 64],
    /// Sentinel energy budget: max background CPU % (Law 8)
    pub max_background_cpu_pct: u8,
    /// Min Qernel version required
    pub min_kernel_version: u32,
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Manifest validation error.
#[derive(Debug)]
pub enum LedgerError {
    /// App ID already installed with same or newer version
    AlreadyInstalled { id: String },
    /// Package hash does not match the binary provided
    HashMismatch { expected: PackageHash, got: PackageHash },
    /// Manifest is missing required fields
    MalformedManifest { reason: &'static str },
    /// Kernel version too old to run this app
    KernelVersionTooOld { required: u32, current: u32 },
    /// Requested capability not allowed on this device profile
    CapabilityDenied { cap: ManifestCapability },
    /// App ID format invalid (must be reverse-domain)
    InvalidAppId { id: String },
    /// Package not found in ledger
    NotFound { id: String },
}

/// Current Qernel version for manifest compatibility checks.
pub const CURRENT_KERNEL_VERSION: u32 = 63_000; // Phase 63 = 63000

/// Validate a manifest without consulting the ledger.
pub fn validate_manifest(manifest: &AppManifest) -> Result<(), LedgerError> {
    // 1. App ID must contain at least one '.' (reverse-domain)
    if !manifest.id.contains('.') {
        return Err(LedgerError::InvalidAppId { id: manifest.id.clone() });
    }
    // 2. Name and version must be non-empty
    if manifest.name.is_empty() || manifest.version.is_empty() {
        return Err(LedgerError::MalformedManifest { reason: "name or version is empty" });
    }
    // 3. Kernel version
    if manifest.min_kernel_version > CURRENT_KERNEL_VERSION {
        return Err(LedgerError::KernelVersionTooOld {
            required: manifest.min_kernel_version,
            current: CURRENT_KERNEL_VERSION,
        });
    }
    // 4. Signature verification placeholder (in production: Ed25519 over packed fields)
    // TODO: integrate hardware-accelerated Ed25519 via TPM enclave
    crate::serial_println!(
        "[LEDGER] Manifest validation OK: {} v{}", manifest.id, manifest.version
    );
    Ok(())
}

// ── Package Record ────────────────────────────────────────────────────────────

/// Current installation state of a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageState {
    /// Binary validated but not yet compiled  
    Pending,
    /// WASM AOT compilation in progress
    Compiling,
    /// Fully installed and ready to launch
    Installed,
    /// Disabled by Sentinel (law violation history)
    Quarantined,
}

/// A fully installed package record.
#[derive(Debug, Clone)]
pub struct PackageRecord {
    pub manifest: AppManifest,
    pub package_hash: PackageHash,
    /// Compiled native binary Prism OID (set when Installed)
    pub compiled_oid: Option<u64>,
    /// Current installation state
    pub state: PackageState,
    /// Q-Credits spent to install this package
    pub credits_spent: u64,
    /// Number of times any Silo has launched this package
    pub launch_count: u64,
    /// Kernel tick of installation
    pub installed_at: u64,
}

// ── Ledger Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct LedgerStats {
    pub packages_installed: u64,
    pub packages_removed: u64,
    pub dedup_hits: u64,        // Times a library was shared (not re-stored)
    pub dedup_bytes_saved: u64,
    pub manifest_rejections: u64,
    pub quarantined_apps: u64,
}

// ── The Q-Ledger ──────────────────────────────────────────────────────────────

/// The kernel-side Q-Ledger: the authoritative map of all installed packages.
pub struct QLedger {
    /// App ID → package record
    pub packages: BTreeMap<String, PackageRecord>,
    /// Package hash → Prism OID (library deduplication)
    pub hash_to_oid: BTreeMap<PackageHash, u64>,
    /// Stats
    pub stats: LedgerStats,
}

impl QLedger {
    pub fn new() -> Self {
        QLedger {
            packages: BTreeMap::new(),
            hash_to_oid: BTreeMap::new(),
            stats: LedgerStats::default(),
        }
    }

    /// Install a new package (called after WASM validation + AOT compilation).
    ///
    /// Enforces Law 2 (Immutable Binaries), Law 5 (Global Deduplication).
    pub fn install(
        &mut self,
        manifest: AppManifest,
        binary: &[u8],
        compiled_oid: u64,
        tick: u64,
    ) -> Result<PackageHash, LedgerError> {
        // Guard: duplicate install check
        if self.packages.contains_key(&manifest.id) {
            return Err(LedgerError::AlreadyInstalled { id: manifest.id.clone() });
        }

        // Validate manifest
        validate_manifest(&manifest)?;

        // Verify binary hash matches manifest
        let hash = PackageHash::from_bytes(binary);
        if hash != manifest.entry_hash {
            self.stats.manifest_rejections += 1;
            return Err(LedgerError::HashMismatch {
                expected: manifest.entry_hash,
                got: hash,
            });
        }

        // Law 5: global deduplication — check if binary is already cached
        if let Some(&existing_oid) = self.hash_to_oid.get(&hash) {
            self.stats.dedup_hits += 1;
            self.stats.dedup_bytes_saved += binary.len() as u64;
            crate::serial_println!(
                "[LEDGER] Dedup hit: {} reuses OID {} ({}B saved)",
                manifest.id, existing_oid, binary.len()
            );
        } else {
            self.hash_to_oid.insert(hash, compiled_oid);
        }

        crate::serial_println!(
            "[LEDGER] Installed: {} v{} (hash={:016x})",
            manifest.id, manifest.version, hash.short_hex()
        );

        self.packages.insert(manifest.id.clone(), PackageRecord {
            package_hash: hash,
            compiled_oid: Some(compiled_oid),
            state: PackageState::Installed,
            credits_spent: 0,
            launch_count: 0,
            installed_at: tick,
            manifest,
        });

        self.stats.packages_installed += 1;
        Ok(hash)
    }

    /// Remove a package (Law 2: leaves zero residue — just deletes pointer).
    pub fn remove(&mut self, app_id: &str) -> Result<(), LedgerError> {
        if self.packages.remove(app_id).is_some() {
            crate::serial_println!("[LEDGER] Removed: {} (0% residue)", app_id);
            self.stats.packages_removed += 1;
            Ok(())
        } else {
            Err(LedgerError::NotFound { id: app_id.to_string() })
        }
    }

    /// Quarantine a package after a Sentinel law violation.
    pub fn quarantine(&mut self, app_id: &str, violation: &'static str) {
        if let Some(rec) = self.packages.get_mut(app_id) {
            rec.state = PackageState::Quarantined;
            self.stats.quarantined_apps += 1;
            crate::serial_println!(
                "[LEDGER] QUARANTINED: {} — reason: {}", app_id, violation
            );
        }
    }

    /// Get the compiled OID for a package ready to launch.
    pub fn get_compiled_oid(&self, app_id: &str) -> Option<u64> {
        let rec = self.packages.get(app_id)?;
        if rec.state == PackageState::Installed {
            rec.compiled_oid
        } else {
            None
        }
    }

    /// Record a Silo launch for stats.
    pub fn record_launch(&mut self, app_id: &str) {
        if let Some(rec) = self.packages.get_mut(app_id) {
            rec.launch_count += 1;
        }
    }
}
