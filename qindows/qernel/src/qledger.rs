//! # Q-Ledger — Application Distribution Ledger
//!
//! The first WebAssembly-Native OS app distribution system (Section 2.3).
//!
//! Key properties:
//! - **Universal Binaries**: Developers ship Wasm blobs; Qindows AOT-compiles
//!   to native x86_64/ARM at install time.
//! - **Atomic Installs**: Apps are read-only, content-addressable, signed
//!   cryptographic hashes on The Ledger.
//! - **Global Deduplication**: Shared libraries stored exactly once.
//!   Uninstalling = deleting a pointer, 0% residue.
//! - **Integrity**: Every binary is Ed25519-signed; Sentinel verifies on load.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Content-addressable hash (SHA-256 of the Wasm binary).
pub type ContentHash = [u8; 32];

/// A developer signing key (Ed25519 public key).
pub type SigningKey = [u8; 32];

/// Binary target architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    /// WebAssembly (portable, not yet compiled)
    Wasm,
    /// Ahead-of-time compiled for x86_64
    X86_64,
    /// Ahead-of-time compiled for AArch64
    Aarch64,
}

/// Install state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    /// Downloaded, not yet installed
    Staged,
    /// AOT compilation in progress
    Compiling,
    /// Installed and ready
    Installed,
    /// Update available
    UpdateAvailable,
    /// Uninstalled (pointer removed, dedup ref decremented)
    Removed,
    /// Signature verification failed
    Rejected,
}

/// A Ledger entry — one application binary.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    /// Content hash (the identity of this binary)
    pub hash: ContentHash,
    /// Application ID (e.g., "org.qindows.collab")
    pub app_id: String,
    /// Human-readable name
    pub name: String,
    /// Version string (semver)
    pub version: String,
    /// Developer's signing key
    pub signing_key: SigningKey,
    /// Ed25519 signature over the content hash
    pub signature: [u8; 64],
    /// Original Wasm binary size
    pub wasm_size: u64,
    /// AOT-compiled native size (0 if not yet compiled)
    pub native_size: u64,
    /// Target architecture
    pub arch: TargetArch,
    /// Install state
    pub state: InstallState,
    /// Dependencies (content hashes of required libraries)
    pub dependencies: Vec<ContentHash>,
    /// Install timestamp
    pub installed_at: u64,
    /// Dedup reference count (how many apps reference this blob)
    pub ref_count: u32,
}

/// A shared library (globally deduplicated).
#[derive(Debug, Clone)]
pub struct SharedLib {
    /// Content hash
    pub hash: ContentHash,
    /// Library name
    pub name: String,
    /// Size in bytes
    pub size: u64,
    /// Reference count (number of apps using this lib)
    pub ref_count: u32,
}

/// Ledger statistics.
#[derive(Debug, Clone, Default)]
pub struct LedgerStats {
    pub apps_installed: u64,
    pub apps_removed: u64,
    pub bytes_deduplicated: u64,
    pub aot_compilations: u64,
    pub signature_failures: u64,
    pub total_wasm_bytes: u64,
    pub total_native_bytes: u64,
}

/// The Q-Ledger.
pub struct QLedger {
    /// All entries by content hash
    pub entries: BTreeMap<ContentHash, LedgerEntry>,
    /// App ID → latest content hash mapping
    pub app_index: BTreeMap<String, ContentHash>,
    /// Shared libraries (dedup pool)
    pub shared_libs: BTreeMap<ContentHash, SharedLib>,
    /// Trusted signing keys
    pub trusted_keys: Vec<SigningKey>,
    /// Statistics
    pub stats: LedgerStats,
}

impl QLedger {
    pub fn new() -> Self {
        QLedger {
            entries: BTreeMap::new(),
            app_index: BTreeMap::new(),
            shared_libs: BTreeMap::new(),
            trusted_keys: Vec::new(),
            stats: LedgerStats::default(),
        }
    }

    /// Add a trusted developer signing key.
    pub fn trust_key(&mut self, key: SigningKey) {
        if !self.trusted_keys.contains(&key) {
            self.trusted_keys.push(key);
        }
    }

    /// Stage a new application (download + verify signature).
    pub fn stage(
        &mut self,
        app_id: &str,
        name: &str,
        version: &str,
        wasm_hash: ContentHash,
        wasm_size: u64,
        signing_key: SigningKey,
        signature: [u8; 64],
        dependencies: Vec<ContentHash>,
        now: u64,
    ) -> Result<ContentHash, &'static str> {
        // Verify the signing key is trusted
        if !self.trusted_keys.contains(&signing_key) {
            self.stats.signature_failures += 1;
            return Err("Untrusted signing key");
        }

        // Verify signature (simplified — production uses Ed25519)
        if !self.verify_signature(&wasm_hash, &signing_key, &signature) {
            self.stats.signature_failures += 1;
            return Err("Invalid signature");
        }

        // Check dedup — if this exact hash exists, just bump ref count
        if let Some(existing) = self.entries.get_mut(&wasm_hash) {
            existing.ref_count = existing.ref_count.saturating_add(1);
            self.stats.bytes_deduplicated += wasm_size;
            return Ok(wasm_hash);
        }

        let entry = LedgerEntry {
            hash: wasm_hash,
            app_id: String::from(app_id),
            name: String::from(name),
            version: String::from(version),
            signing_key,
            signature,
            wasm_size,
            native_size: 0,
            arch: TargetArch::Wasm,
            state: InstallState::Staged,
            dependencies,
            installed_at: now,
            ref_count: 1,
        };

        self.entries.insert(wasm_hash, entry);
        self.stats.total_wasm_bytes += wasm_size;
        Ok(wasm_hash)
    }

    /// AOT-compile a staged Wasm binary to native code.
    pub fn compile(&mut self, hash: &ContentHash) -> Result<(), &'static str> {
        let entry = self.entries.get_mut(hash)
            .ok_or("Entry not found")?;

        if entry.state != InstallState::Staged {
            return Err("Entry not in staged state");
        }

        entry.state = InstallState::Compiling;

        // In production: invoke Cranelift/LLVM to compile Wasm → native
        // Estimated: native code is typically 1.5x the Wasm bytecode size
        let native_size = entry.wasm_size * 3 / 2;
        entry.native_size = native_size;
        entry.arch = TargetArch::X86_64;
        entry.state = InstallState::Installed;

        self.stats.aot_compilations += 1;
        self.stats.total_native_bytes += native_size;
        self.stats.apps_installed += 1;

        // Update app index
        let app_id = entry.app_id.clone();
        self.app_index.insert(app_id, *hash);

        Ok(())
    }

    /// Uninstall an application (delete pointer, decrement dedup refs).
    pub fn uninstall(&mut self, app_id: &str) -> Result<u64, &'static str> {
        let hash = self.app_index.remove(app_id)
            .ok_or("App not found")?;

        let mut freed = 0u64;

        // Collect dependencies and sizes before potentially removing entry
        let (ref_count, deps, wasm_size, native_size) = match self.entries.get_mut(&hash) {
            Some(entry) => {
                entry.ref_count = entry.ref_count.saturating_sub(1);
                entry.state = InstallState::Removed;
                (entry.ref_count, entry.dependencies.clone(), entry.wasm_size, entry.native_size)
            }
            None => return Ok(0),
        };

        if ref_count == 0 {
            // Last reference — actually free the storage
            freed = native_size + wasm_size;
            self.entries.remove(&hash);
        }

        // Decrement shared lib references (using pre-collected deps)
        for dep in &deps {
            if let Some(lib) = self.shared_libs.get_mut(dep) {
                lib.ref_count = lib.ref_count.saturating_sub(1);
                if lib.ref_count == 0 {
                    freed += lib.size;
                }
            }
        }

        self.stats.apps_removed += 1;
        Ok(freed)
    }

    /// Verify a signature (simplified — production uses Ed25519).
    fn verify_signature(&self, hash: &ContentHash, key: &SigningKey, sig: &[u8; 64]) -> bool {
        // Simplified: check that sig[0..32] XOR hash == key
        // Production: use proper Ed25519 verification
        let mut check = [0u8; 32];
        for i in 0..32 {
            check[i] = sig[i] ^ hash[i];
        }
        check == *key
    }

    /// Look up an app by ID.
    pub fn lookup(&self, app_id: &str) -> Option<&LedgerEntry> {
        let hash = self.app_index.get(app_id)?;
        self.entries.get(hash)
    }

    /// Register a shared library in the dedup pool.
    pub fn register_shared_lib(&mut self, hash: ContentHash, name: &str, size: u64) {
        self.shared_libs.entry(hash).or_insert_with(|| SharedLib {
            hash,
            name: String::from(name),
            size,
            ref_count: 0,
        }).ref_count += 1;
    }

    /// Get total storage saved by deduplication.
    pub fn dedup_savings(&self) -> u64 {
        self.stats.bytes_deduplicated
    }
}
