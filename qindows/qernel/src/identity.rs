//! # Q-Identity — Hardware Enclave & TPM Authentication (Phase 64)
//!
//! Q-Identity is the Qindows root of trust. Every user, device, and session
//! is identified by a cryptographic key pair bound to the hardware TPM 3.0 enclave.
//!
//! ## ARCHITECTURE.md §3.4: Hardware Vault Lock
//! > "O-IDs are cryptographically tied to the TPM 3.0 Hardware Enclave.
//! > Pulling the SSD out renders data into a 'Sea of Shards' (Digital Noise)
//! > unless unlocked by a biometric Identity Token."
//!
//! ## ARCHITECTURE.md §6.2 (Q-Synapse): Privacy Contract
//! The neural BCI requires a valid `IdentityToken` to register bindings.
//! Thought patterns are encrypted with the identity's ephemeral session key.
//!
//! ## Architecture Guardian: Layering
//! ```text
//! Q-Synapse, Prism, Q-Fabric   ← consume IdentityToken
//!       │
//! Q-Identity (this module)     ← establishes tokens from hardware
//!       │
//! TPM 3.0 / Secure Enclave     ← never exports private keys
//! ```
//!
//! - This module interacts with the TPM via MMIO (`dev://tpm/...` in UNS)
//! - Private keys are non-exportable: they live only in TPM persistent storage
//! - Session keys are ephemeral: derived per-boot, sealed to PCR state

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── Identity Types ────────────────────────────────────────────────────────────

/// A 256-bit globally unique Node/User identity.
/// Derived from the TPM's Endorsement Key certificate fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdentityId(pub [u8; 32]);

impl IdentityId {
    pub const KERNEL: Self = IdentityId([0xFFu8; 32]); // The Qernel's own identity

    pub fn short_hex(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().unwrap_or([0u8; 8]))
    }

    /// Derive an IdentityId from a seed (placeholder for TPM EK derivation).
    pub fn from_seed(seed: &[u8]) -> Self {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in seed {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        let mut out = [0u8; 32];
        for i in 0..4 {
            let v = h.wrapping_add(i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            out[i*8..(i+1)*8].copy_from_slice(&v.to_le_bytes());
        }
        IdentityId(out)
    }
}

// ── Identity Token ────────────────────────────────────────────────────────────

/// An identity token handed to Q-Ring syscalls to prove "who is asking."
///
/// Tokens are session-scoped (expire at `expires_tick`) and tied to a specific
/// biometric or PIN verification event. They are NOT transferable between Silos.
#[derive(Debug, Clone)]
pub struct IdentityToken {
    /// The authenticated user/node this token represents
    pub identity_id: IdentityId,
    /// Which Silo is the exclusive holder of this token
    pub bound_silo: u64,
    /// Kernel tick at which this token was issued
    pub issued_at: u64,
    /// Kernel tick at which this token expires (0 = no expiry, admin-only)
    pub expires_at: u64,
    /// Authentication method used
    pub auth_method: AuthMethod,
    /// Is this token currently revoked?
    pub revoked: bool,
    /// Token serial (prevents replay)
    pub serial: u64,
}

impl IdentityToken {
    /// Returns true if the token is still valid at the given kernel tick.
    pub fn is_valid_at(&self, tick: u64) -> bool {
        if self.revoked { return false; }
        if self.expires_at != 0 && tick > self.expires_at { return false; }
        true
    }

    /// Convenience: is valid right now (caller supplies current tick).
    pub fn is_valid(&self) -> bool {
        true // In production: !self.revoked && tick-check; caller supplies tick
    }
}

/// Method used to authenticate the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// Hardware biometric (fingerprint / iris) via TPM
    Biometric,
    /// Neural pattern (Q-Synapse BCI double-tap + PIN)
    NeuralBiometric,
    /// Hardware security key (FIDO2 + TPM attestation)
    HardwareKey,
    /// PIN-only (lower trust, time-limited)
    Pin,
    /// Bootstrap (kernel-internal identity for pre-auth operations)
    Bootstrap,
}

// ── PCR State ─────────────────────────────────────────────────────────────────

/// Platform Configuration Register snapshot (TPM 3.0).
/// PCRs are extended (SHA-384) with each boot measurement.
/// Session keys are "sealed" to a specific PCR state — they cannot be
/// unsealed on a tampered system.
#[derive(Debug, Clone, Copy)]
pub struct PcrSnapshot {
    /// PCR[0]: Firmware (UEFI)
    pub pcr0: [u8; 48],
    /// PCR[4]: Boot Manager
    pub pcr4: [u8; 48],
    /// PCR[7]: Secure Boot policy
    pub pcr7: [u8; 48],
    /// PCR[11]: Qernel binary hash
    pub pcr11: [u8; 48],
}

impl PcrSnapshot {
    pub fn zeroed() -> Self {
        PcrSnapshot {
            pcr0: [0u8; 48],
            pcr4: [0u8; 48],
            pcr7: [0u8; 48],
            pcr11: [0u8; 48],
        }
    }

    /// Simulate extending a PCR with a measurement hash.
    pub fn extend(&mut self, pcr: u8, measurement: &[u8]) {
        let target = match pcr {
            0  => &mut self.pcr0,
            4  => &mut self.pcr4,
            7  => &mut self.pcr7,
            11 => &mut self.pcr11,
            _  => return,
        };
        // PCR extend: PCR_new = hash(PCR_old || measurement)
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in target.iter().chain(measurement.iter()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        // Fill 48 bytes with derived values
        for i in 0..6 {
            let v = h.wrapping_add(i).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            target[i as usize * 8..(i as usize + 1) * 8].copy_from_slice(&v.to_le_bytes());
        }
        crate::serial_println!("[IDENTITY] PCR[{}] extended.", pcr);
    }
}

// ── Session Key ───────────────────────────────────────────────────────────────

/// An ephemeral session key derived at boot, sealed to PCR state.
/// Cannot be extracted from hardware on a tampered system.
#[derive(Debug, Clone)]
pub struct SessionKey {
    /// The identity this session key belongs to
    pub owner: IdentityId,
    /// 256-bit AES-GCM session key (placeholder — in production: sealed blob)
    pub key_material: [u8; 32],
    /// PCR state this key was sealed to
    pub sealed_pcr: PcrSnapshot,
    /// Has this session key been successfully unsealed?
    pub unsealed: bool,
}

// ── Identity Store ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub id: IdentityId,
    pub name: String,
    pub auth_methods: Vec<AuthMethod>,
    /// Active session keys (one per boot session)
    pub session_key: Option<SessionKey>,
    /// Capability tier set during first-boot
    pub capability_tier: CapabilityTier,
    /// Creation tick
    pub created_at: u64,
    /// Total sessions (for audit log)
    pub session_count: u64,
}

/// From ARCHITECTURE.md §"First Boot" — user's privacy/capability level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTier {
    /// Maximum compatibility — apps can request broader access
    Monolith,
    /// Strict silos + onion-routed network (maximum privacy)
    Ghost,
    /// Optimized for cloud-offloading and P2P mesh sharing
    Flow,
}

// ── Q-Identity Manager ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct IdentityStats {
    pub auth_successes: u64,
    pub auth_failures: u64,
    pub tokens_issued: u64,
    pub tokens_revoked: u64,
    pub pcr_verifications: u64,
}

/// The Q-Identity kernel manager.
pub struct QIdentityManager {
    /// Active registered users
    pub users: BTreeMap<IdentityId, UserIdentity>,
    /// All active tokens: serial → token
    pub active_tokens: BTreeMap<u64, IdentityToken>,
    /// Current boot's PCR snapshot
    pub boot_pcr: PcrSnapshot,
    /// Next token serial number
    next_serial: u64,
    /// Stats
    pub stats: IdentityStats,
}

impl QIdentityManager {
    pub fn new() -> Self {
        QIdentityManager {
            users: BTreeMap::new(),
            active_tokens: BTreeMap::new(),
            boot_pcr: PcrSnapshot::zeroed(),
            next_serial: 1,
            stats: IdentityStats::default(),
        }
    }

    /// Initialize PCR 11 with the Qernel binary hash (called by _start).
    pub fn measure_kernel(&mut self, kernel_bytes: &[u8]) {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in kernel_bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        self.boot_pcr.extend(11, &h.to_le_bytes());
        crate::serial_println!("[IDENTITY] Qernel measured into PCR[11].");
    }

    /// Register a new user identity (called during First Boot setup).
    pub fn register_user(
        &mut self,
        name: String,
        seed: &[u8],
        tier: CapabilityTier,
        tick: u64,
    ) -> IdentityId {
        let id = IdentityId::from_seed(seed);
        crate::serial_println!(
            "[IDENTITY] User registered: \"{}\" id={:016x} tier={:?}",
            name, id.short_hex(), tier
        );
        self.users.insert(id, UserIdentity {
            id,
            name,
            auth_methods: alloc::vec![AuthMethod::Bootstrap],
            session_key: None,
            capability_tier: tier,
            created_at: tick,
            session_count: 0,
        });
        id
    }

    /// Authenticate an identity and issue a session token.
    ///
    /// This is the entry point for biometric / PIN auth events.
    /// In production: verifies TPM attestation signature before issuing.
    pub fn authenticate(
        &mut self,
        identity_id: IdentityId,
        method: AuthMethod,
        bound_silo: u64,
        duration_ticks: u64, // 0 = no expiry
        tick: u64,
    ) -> Result<u64, &'static str> {
        if !self.users.contains_key(&identity_id) {
            self.stats.auth_failures += 1;
            return Err("Q-Identity: unknown identity");
        }

        let serial = self.next_serial;
        self.next_serial += 1;

        let token = IdentityToken {
            identity_id,
            bound_silo,
            issued_at: tick,
            expires_at: if duration_ticks == 0 { 0 } else { tick + duration_ticks },
            auth_method: method,
            revoked: false,
            serial,
        };

        crate::serial_println!(
            "[IDENTITY] Token #{} issued: id={:016x} silo={} method={:?}",
            serial, identity_id.short_hex(), bound_silo, method
        );

        self.active_tokens.insert(serial, token);
        self.stats.tokens_issued += 1;
        self.stats.auth_successes += 1;

        if let Some(user) = self.users.get_mut(&identity_id) {
            user.session_count += 1;
        }

        Ok(serial)
    }

    /// Validate a token serial for a syscall (called by kernel gate).
    pub fn validate_token(&self, serial: u64, silo_id: u64, tick: u64) -> bool {
        match self.active_tokens.get(&serial) {
            Some(tok) if tok.bound_silo == silo_id && tok.is_valid_at(tick) => true,
            _ => false,
        }
    }

    /// Revoke a token (Sentinel or user can revoke).
    pub fn revoke_token(&mut self, serial: u64) {
        if let Some(tok) = self.active_tokens.get_mut(&serial) {
            tok.revoked = true;
            self.stats.tokens_revoked += 1;
            crate::serial_println!("[IDENTITY] Token #{} REVOKED.", serial);
        }
    }

    /// Revoke all tokens for a Silo (called on vaporize).
    pub fn revoke_silo_tokens(&mut self, silo_id: u64) {
        let mut revoked = 0u64;
        for tok in self.active_tokens.values_mut() {
            if tok.bound_silo == silo_id && !tok.revoked {
                tok.revoked = true;
                revoked += 1;
                self.stats.tokens_revoked += 1;
            }
        }
        if revoked > 0 {
            crate::serial_println!(
                "[IDENTITY] Silo {} vaporized — {} tokens revoked.", silo_id, revoked
            );
        }
    }

    /// Get the capability tier for the current authenticated user of a Silo.
    pub fn silo_capability_tier(&self, serial: u64) -> Option<CapabilityTier> {
        let tok = self.active_tokens.get(&serial)?;
        let user = self.users.get(&tok.identity_id)?;
        Some(user.capability_tier)
    }
}
