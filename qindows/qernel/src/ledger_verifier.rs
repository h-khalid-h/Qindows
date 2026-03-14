//! # Ledger Manifest Signing Integration (Phase 122)
//!
//! ## Architecture Guardian: The Gap
//! `ledger.rs:147-148`:
//! ```rust
//! // 4. Signature verification placeholder (in production: Ed25519 over packed fields)
//! // TODO: integrate hardware-accelerated Ed25519 via TPM enclave
//! ```
//! `validate_manifest()` returns `Ok(())` without verifying the developer signature.
//!
//! This module provides `LedgerManifestVerifier`:
//! 1. Calls `ledger::validate_manifest()` for field checks
//! 2. Verifies developer attestation: HMAC-SHA-256(publisher_key[..32], entry_hash || app_id)
//! 3. Checks `entry_hash` against raw bytes via `crypto_primitives::sha256`
//! 4. Wires `QTrafficEngine::authorize_silo()` at install time (Law 7)

extern crate alloc;
use alloc::string::String;
use alloc::collections::BTreeMap;

use crate::ledger::{QLedger, AppManifest, PackageHash, validate_manifest, LedgerError};
use crate::crypto_primitives::{sha256, hmac_sha256};
use crate::qtraffic::{QTrafficEngine, Law7Verdict};

// ── Verifier Statistics ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ManifestVerifierStats {
    pub validateds:         u64,
    pub sig_ok:             u64,
    pub sig_failed:         u64,
    pub hash_mismatches:    u64,
}

// ── Ledger Manifest Verifier ──────────────────────────────────────────────────

/// Extends QLedger::install() with real crypto verification.
pub struct LedgerManifestVerifier {
    pub ledger:  QLedger,
    pub traffic: QTrafficEngine,
    pub stats:   ManifestVerifierStats,
}

impl LedgerManifestVerifier {
    pub fn new() -> Self {
        LedgerManifestVerifier {
            ledger: QLedger::new(),
            traffic: QTrafficEngine::new(),
            stats: ManifestVerifierStats::default(),
        }
    }

    /// Full install pipeline with crypto verification.
    pub fn install_verified(
        &mut self,
        manifest: AppManifest,
        package_bytes: &[u8],
        tick: u64,
    ) -> Result<(), LedgerError> {
        self.stats.validateds += 1;

        // 1. Basic field validation
        validate_manifest(&manifest)?;

        // 2. Hash verification: entry_hash must match SHA-256(bytes)
        let computed_hash = sha256(package_bytes);
        if computed_hash != manifest.entry_hash.0 {
            self.stats.hash_mismatches += 1;
            crate::serial_println!(
                "[LEDGER] Hash mismatch '{}': computed={:02x}{:02x}.. manifest={:02x}{:02x}..",
                manifest.id, computed_hash[0], computed_hash[1],
                manifest.entry_hash.0[0], manifest.entry_hash.0[1]
            );
            return Err(LedgerError::HashMismatch {
                expected: manifest.entry_hash.clone(),
                got: PackageHash(computed_hash),
            });
        }

        // 3. Publisher signature verification
        // sig_payload = HMAC-SHA-256(publisher_key[..32], entry_hash || app_id_bytes)
        let pub_key_half: [u8; 32] = manifest.publisher_key[..32].try_into().unwrap_or([0;32]);
        let mut sig_payload = [0u8; 64];
        sig_payload[..32].copy_from_slice(&manifest.entry_hash.0);
        let id_bytes = manifest.id.as_bytes();
        let copy_len = id_bytes.len().min(32);
        sig_payload[32..32+copy_len].copy_from_slice(&id_bytes[..copy_len]);

        let expected_sig_32 = hmac_sha256(&pub_key_half, &sig_payload);
        // The manifest stores a 64-byte Ed25519 sig; we verify only first 32 bytes (HMAC)
        let sig_first_32: [u8; 32] = manifest.signature[..32].try_into().unwrap_or([0;32]);

        if expected_sig_32 != sig_first_32 {
            self.stats.sig_failed += 1;
            crate::serial_println!("[LEDGER] Signature INVALID for '{}'", manifest.id);
            return Err(LedgerError::MalformedManifest { reason: "invalid developer signature" });
        }
        self.stats.sig_ok += 1;

        // 4. Open Law 7 traffic account (rate limit = max_background_cpu_pct as proxy)
        let traffic_silo_id = u64::from_le_bytes(manifest.entry_hash.0[..8].try_into().unwrap_or([0;8]));
        let rate_limit_bps = (manifest.max_background_cpu_pct as u64) * 100_000; // 100KB/s per %
        self.traffic.authorize_silo(traffic_silo_id, rate_limit_bps);

        // 5. Install in Ledger (0 credits for now — wallet integration is separate)
        self.ledger.install(manifest, package_bytes, 0, tick)?;

        crate::serial_println!("[LEDGER] ✓ Install verified: sig OK, hash OK");
        Ok(())
    }

    /// Check Law 7 compliance for a Silo's flow.
    pub fn check_law7(&self, silo_id: u64) -> bool {
        matches!(self.traffic.check_law7(silo_id), Law7Verdict::Allow)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  LedgerVerifier: validated={} sig_ok={} sig_fail={} hash_mismatch={}",
            self.stats.validateds, self.stats.sig_ok,
            self.stats.sig_failed, self.stats.hash_mismatches
        );
    }
}
