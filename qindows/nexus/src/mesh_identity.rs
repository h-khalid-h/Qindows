//! # Mesh Identity — Device Identity & Attestation
//!
//! Every device on the Global Mesh has a cryptographic identity
//! (Section 11.2). Before a device can contribute compute or
//! receive offloaded tasks, it must attest its hardware integrity.
//!
//! Features:
//! - Ed25519 device keypair
//! - TPM-backed hardware attestation
//! - Mutual authentication handshake
//! - Identity revocation via Sentinel
//! - Reputation score (based on uptime, reliability, fraud history)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Attestation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestState {
    /// Not yet attested
    Unverified,
    /// Attestation in progress
    Pending,
    /// Successfully attested
    Verified,
    /// Attestation failed (bad hardware / tampered)
    Failed,
    /// Revoked (by Sentinel)
    Revoked,
}

/// A device identity on the mesh.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    /// Device ID (Ed25519 public key)
    pub id: [u8; 32],
    /// Device name
    pub name: String,
    /// Attestation state
    pub attest: AttestState,
    /// TPM quote hash
    pub tpm_quote: [u8; 32],
    /// Hardware fingerprint
    pub hw_fingerprint: [u8; 32],
    /// First seen timestamp
    pub first_seen: u64,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Reputation score (0-100)
    pub reputation: u8,
    /// Uptime percentage (0-100)
    pub uptime_pct: u8,
    /// Successful task completions
    pub tasks_completed: u64,
    /// Failed/abandoned tasks
    pub tasks_failed: u64,
}

/// An attestation challenge/response.
#[derive(Debug, Clone)]
pub struct AttestChallenge {
    pub challenge_id: u64,
    pub device_id: [u8; 32],
    pub nonce: [u8; 32],
    pub created_at: u64,
    pub expires_at: u64,
    pub response: Option<[u8; 64]>,
}

/// Identity statistics.
#[derive(Debug, Clone, Default)]
pub struct IdentityStats {
    pub devices_registered: u64,
    pub attestations_passed: u64,
    pub attestations_failed: u64,
    pub devices_revoked: u64,
    pub challenges_issued: u64,
}

/// The Mesh Identity Manager.
pub struct MeshIdentity {
    pub devices: BTreeMap<[u8; 32], DeviceIdentity>,
    pub challenges: BTreeMap<u64, AttestChallenge>,
    next_challenge_id: u64,
    /// Minimum reputation to participate in mesh
    pub min_reputation: u8,
    /// Challenge validity duration (seconds)
    pub challenge_ttl: u64,
    pub stats: IdentityStats,
}

impl MeshIdentity {
    pub fn new() -> Self {
        MeshIdentity {
            devices: BTreeMap::new(),
            challenges: BTreeMap::new(),
            next_challenge_id: 1,
            min_reputation: 30,
            challenge_ttl: 60,
            stats: IdentityStats::default(),
        }
    }

    /// Register a new device.
    pub fn register(&mut self, id: [u8; 32], name: &str, hw_fp: [u8; 32], now: u64) {
        self.devices.entry(id).or_insert_with(|| {
            self.stats.devices_registered += 1;
            DeviceIdentity {
                id, name: String::from(name),
                attest: AttestState::Unverified,
                tpm_quote: [0; 32], hw_fingerprint: hw_fp,
                first_seen: now, last_seen: now,
                reputation: 50, uptime_pct: 100,
                tasks_completed: 0, tasks_failed: 0,
            }
        });
    }

    /// Issue an attestation challenge.
    pub fn challenge(&mut self, device_id: [u8; 32], nonce: [u8; 32], now: u64) -> Option<u64> {
        if !self.devices.contains_key(&device_id) { return None; }

        let id = self.next_challenge_id;
        self.next_challenge_id += 1;

        self.challenges.insert(id, AttestChallenge {
            challenge_id: id, device_id, nonce,
            created_at: now,
            expires_at: now + self.challenge_ttl,
            response: None,
        });

        self.stats.challenges_issued += 1;
        Some(id)
    }

    /// Verify an attestation response.
    pub fn verify(&mut self, challenge_id: u64, response: [u8; 64], tpm_quote: [u8; 32], now: u64) -> Result<(), &'static str> {
        let challenge = self.challenges.get_mut(&challenge_id)
            .ok_or("Challenge not found")?;

        if now > challenge.expires_at {
            return Err("Challenge expired");
        }

        challenge.response = Some(response);

        // In production: verify Ed25519 signature over (nonce || tpm_quote)
        // Simplified: accept if tpm_quote is non-zero
        let valid = tpm_quote.iter().any(|&b| b != 0);

        if let Some(device) = self.devices.get_mut(&challenge.device_id) {
            if valid {
                device.attest = AttestState::Verified;
                device.tpm_quote = tpm_quote;
                device.last_seen = now;
                self.stats.attestations_passed += 1;
            } else {
                device.attest = AttestState::Failed;
                device.reputation = device.reputation.saturating_sub(20);
                self.stats.attestations_failed += 1;
            }
        }

        Ok(())
    }

    /// Update reputation after task completion.
    pub fn task_complete(&mut self, device_id: &[u8; 32], success: bool) {
        if let Some(device) = self.devices.get_mut(device_id) {
            if success {
                device.tasks_completed += 1;
                device.reputation = device.reputation.saturating_add(1).min(100);
            } else {
                device.tasks_failed += 1;
                device.reputation = device.reputation.saturating_sub(5);
            }
        }
    }

    /// Revoke a device (Sentinel enforcement).
    pub fn revoke(&mut self, device_id: &[u8; 32]) {
        if let Some(device) = self.devices.get_mut(device_id) {
            device.attest = AttestState::Revoked;
            device.reputation = 0;
            self.stats.devices_revoked += 1;
        }
    }

    /// Get all verified devices above minimum reputation.
    pub fn trusted_devices(&self) -> Vec<&DeviceIdentity> {
        self.devices.values()
            .filter(|d| d.attest == AttestState::Verified && d.reputation >= self.min_reputation)
            .collect()
    }
}
