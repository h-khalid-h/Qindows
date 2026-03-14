//! # Identity TPM Bridge (Phase 117)
//!
//! ## Architecture Guardian: The Gap
//! `identity.rs` (Phase 71) implements `NodeIdentity` with:
//! - `derive()` — creates identity from a seed (placeholder for TPM EK derivation)
//! - `seal_session_key()` — XOR placeholder for TPM PCR sealing
//! - `attest()` — placeholder Ed25519 signature
//!
//! `digital_antibody.rs` (Phase 74) needs identity attestation to validate
//! module signatures.
//!
//! **Missing link**: Nothing connected the identity's **attestation** to
//! the **crypto_primitives** module (Phase 113). The XOR placeholder in
//! `digital_antibody.rs:328` was never replaced with a real signing call.
//!
//! This module provides `IdentityTpmBridge`:
//! 1. `derive_boot_identity()` — stable 256-bit node ID from HW sources
//! 2. `attest_binary()` — SHA-256 of ELF + HMAC-SHA-256 signature using node key
//! 3. `verify_attestation()` — verifies an attestation from a remote node
//! 4. `seal_cap_token_key()` — derives per-Silo cap-signing keys from node key
//!
//! ## Key Derivation Hierarchy
//! ```text
//! TPM EK (hardware root) or CPUID+tick seed
//!   └─ NodeMasterKey (256-bit)
//!       ├─ BootMeasurementKey (HMAC-SHA-256 with label "BOOT")
//!       ├─ AttestationKey(label "ATTEST")
//!       └─ CapTokenKey[silo_id] (HMAC-SHA-256 with label "CAP:{silo_id}")
//! ```

extern crate alloc;
use alloc::string::String;
use alloc::format;

use crate::crypto_primitives::{sha256, hmac_sha256, binary_oid, cap_token_tag};

// ── Identity Constants ────────────────────────────────────────────────────────

const KDF_LABEL_BOOT:    &[u8] = b"QINDOWS:BOOT_MEASUREMENT:v1";
const KDF_LABEL_ATTEST:  &[u8] = b"QINDOWS:ATTESTATION:v1";
const KDF_LABEL_CAP:     &[u8] = b"QINDOWS:CAP_TOKEN:v1";

// ── Attestation Record ────────────────────────────────────────────────────────

/// Attestation of a binary or kernel module.
#[derive(Debug, Clone)]
pub struct Attestation {
    /// SHA-256 of ELF content
    pub binary_hash: [u8; 32],
    /// HMAC-SHA-256(attestation_key, binary_hash || tick_le)
    pub signature: [u8; 32],
    /// The node that signed this attestation
    pub signer_id: [u8; 32],
    /// Kernel tick of attestation
    pub tick: u64,
}

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TpmBridgeStats {
    pub attestations_created: u64,
    pub attestations_verified: u64,
    pub attestations_failed: u64,
    pub cap_keys_derived: u64,
}

// ── Identity TPM Bridge ───────────────────────────────────────────────────────

/// Provides crypto-backed identity operations for Qindows.
pub struct IdentityTpmBridge {
    /// Node master key (256-bit, derived from hardware entropy at boot).
    pub master_key: [u8; 32],
    /// Stable node identity (public-facing ID, SHA-256 of master_key).
    pub node_id: [u8; 32],
    pub stats: TpmBridgeStats,
}

impl IdentityTpmBridge {
    /// Derive a node identity from hardware entropy.
    /// In production: this reads the TPM Endorsement Key certificate.
    /// Here: HMAC-SHA-256(CPUID || APIC-ID || tick) as a stable entropy seed.
    pub fn from_hardware_entropy(cpuid_leaf0: u64, apic_id: u32, boot_tick: u64) -> Self {
        // Construct entropy buffer
        let mut entropy = [0u8; 20];
        entropy[..8].copy_from_slice(&cpuid_leaf0.to_le_bytes());
        entropy[8..12].copy_from_slice(&apic_id.to_le_bytes());
        entropy[12..20].copy_from_slice(&boot_tick.to_le_bytes());

        // Master key = SHA-256(entropy || "QINDOWS:MASTER:v1")
        let label = b"QINDOWS:MASTER:v1";
        let mut seed = [0u8; 37]; // 20 + 17
        seed[..20].copy_from_slice(&entropy);
        seed[20..].copy_from_slice(label);
        let master_key = sha256(&seed);

        // Node public ID = SHA-256(master_key)
        let node_id = sha256(&master_key);

        crate::serial_println!(
            "[IDENTITY] Node ID derived: {:02x}{:02x}{:02x}{:02x}..",
            node_id[0], node_id[1], node_id[2], node_id[3]
        );

        IdentityTpmBridge {
            master_key,
            node_id,
            stats: TpmBridgeStats::default(),
        }
    }

    /// Derive the attestation key (domain-separated from master key).
    fn attestation_key(&self) -> [u8; 32] {
        hmac_sha256(&self.master_key, KDF_LABEL_ATTEST)
    }

    /// Derive the boot measurement key.
    fn boot_measurement_key(&self) -> [u8; 32] {
        hmac_sha256(&self.master_key, KDF_LABEL_BOOT)
    }

    /// Attest a binary OID (ELF hash) with HMAC-SHA-256 signature.
    /// Replaces the XOR placeholder in `digital_antibody.rs:328`.
    pub fn attest_binary(&mut self, elf_bytes: &[u8], tick: u64) -> Attestation {
        self.stats.attestations_created += 1;
        let binary_hash = binary_oid(elf_bytes);
        let attest_key = self.attestation_key();

        // signature = HMAC-SHA-256(attest_key, binary_hash || tick_le)
        let mut msg = [0u8; 40]; // 32 + 8
        msg[..32].copy_from_slice(&binary_hash);
        msg[32..40].copy_from_slice(&tick.to_le_bytes());
        let signature = hmac_sha256(&attest_key, &msg);

        crate::serial_println!(
            "[IDENTITY] Attested binary {:02x}{:02x}.. sig={:02x}{:02x}..",
            binary_hash[0], binary_hash[1], signature[0], signature[1]
        );

        Attestation {
            binary_hash,
            signature,
            signer_id: self.node_id,
            tick,
        }
    }

    /// Verify an attestation from a remote node.
    /// Returns true if the HMAC is valid for the given binary_hash.
    pub fn verify_attestation(&mut self, att: &Attestation, signer_master_key: &[u8; 32]) -> bool {
        self.stats.attestations_verified += 1;

        // Recompute the attestation key for the remote signer
        let remote_attest_key = hmac_sha256(signer_master_key, KDF_LABEL_ATTEST);

        let mut msg = [0u8; 40];
        msg[..32].copy_from_slice(&att.binary_hash);
        msg[32..40].copy_from_slice(&att.tick.to_le_bytes());
        let expected_sig = hmac_sha256(&remote_attest_key, &msg);

        let ok = expected_sig == att.signature;
        if !ok { self.stats.attestations_failed += 1; }
        ok
    }

    /// Derive a per-Silo CapToken signing key.
    /// Replaces the XOR stub in `cap_tokens.rs`.
    pub fn silo_cap_key(&mut self, silo_id: u64) -> [u8; 32] {
        self.stats.cap_keys_derived += 1;
        let mut label = [0u8; KDF_LABEL_CAP.len() + 8];
        label[..KDF_LABEL_CAP.len()].copy_from_slice(KDF_LABEL_CAP);
        label[KDF_LABEL_CAP.len()..].copy_from_slice(&silo_id.to_le_bytes());
        hmac_sha256(&self.master_key, &label)
    }

    /// Create a CapToken authentication tag for a Silo.
    pub fn sign_cap_token(
        &mut self,
        silo_id: u64,
        cap_type: u8,
        object_oid: &[u8; 32],
        expiry_tick: u64,
    ) -> [u8; 32] {
        let silo_key = self.silo_cap_key(silo_id);
        cap_token_tag(&silo_key, cap_type, object_oid, expiry_tick)
    }

    /// Compute the boot measurement (PCR-0 equivalent).
    pub fn measure_boot(&self, boot_log_bytes: &[u8]) -> [u8; 32] {
        let bmk = self.boot_measurement_key();
        hmac_sha256(&bmk, boot_log_bytes)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IdentityTPM: node={:02x}{:02x}.. attested={} verified={} failed={} caps={}",
            self.node_id[0], self.node_id[1],
            self.stats.attestations_created, self.stats.attestations_verified,
            self.stats.attestations_failed, self.stats.cap_keys_derived
        );
    }
}
