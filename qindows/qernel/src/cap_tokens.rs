//! # CapToken Forge (Phase 119)
//!
//! ## Architecture Guardian: The Gap
//! `identity_tpm_bridge.rs` (Phase 117) provides `sign_cap_token()` showing
//! how to derive a per-Silo CapToken signing key.
//!
//! But there was no `CapToken` struct or **forge** module that actually:
//! 1. Defines the CapToken format (rights, expiry, object scope)
//! 2. Mints new tokens for Silos at spawn
//! 3. Validates tokens on every Q-Ring syscall entry
//! 4. Revokes tokens on Silo vaporize
//!
//! This module is `cap_tokens.rs` — the canonical CapToken runtime.
//!
//! ## CapToken Format
//! Each CapToken = 64 bytes:
//! ```text
//! [0..4]  cap_type u32        (object class: Prism/Aether/IPC/Net/Admin)
//! [4..12] object_oid_prefix   (first 8 bytes of OID scope; 0 = any)
//! [12..20] expiry_tick u64    (0 = non-expiring)
//! [20..28] silo_id u64        (token bound to this Silo)
//! [28..32] flags u32          (READ=1, WRITE=2, EXEC=4, DELEGATE=8)
//! [32..64] signature [u8;32]  (HMAC-SHA-256 via identity_tpm_bridge)
//! ```
//!
//! ## Law 1 (Zero-Ambient Authority)
//! All 26 syscalls check CapToken validity before executing.
//! Only the kernel may mint tokens — Silos CANNOT self-elevate.
//! Delegation (DELEGATE flag) is explicit and rate-limited.

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use crate::crypto_primitives::hmac_sha256;

// ── CapToken Type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CapType {
    None    = 0,
    Prism   = 1,   // Object store read/write
    Aether  = 2,   // UI/compositor blit
    Ipc     = 3,   // Inter-Silo messages
    Network = 4,   // Nexus fabric send
    Admin   = 5,   // Sentinel/kstate access (kernel only)
    Wasm    = 6,   // WASM module execution
    Collab  = 7,   // Collaborative session
    Synapse = 8,   // Neural intent pipeline
    Energy  = 9,   // Energy policy hints
}

impl From<u32> for CapType {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::Prism,   2 => Self::Aether,  3 => Self::Ipc,
            4 => Self::Network, 5 => Self::Admin,   6 => Self::Wasm,
            7 => Self::Collab,  8 => Self::Synapse,  9 => Self::Energy,
            _ => Self::None,
        }
    }
}

// ── CapToken Flags ────────────────────────────────────────────────────────────

pub const CAP_READ:     u32 = 0x01;
pub const CAP_WRITE:    u32 = 0x02;
pub const CAP_EXEC:     u32 = 0x04;
pub const CAP_DELEGATE: u32 = 0x08;
pub const CAP_ALL:      u32 = CAP_READ | CAP_WRITE | CAP_EXEC;

// ── CapToken Struct ───────────────────────────────────────────────────────────

/// A minted capability token (64 bytes on the wire, padded to 96 for alignment).
#[derive(Debug, Clone)]
pub struct CapToken {
    pub cap_type:         CapType,
    pub object_oid_prefix: u64,     // 0 = any object
    pub expiry_tick:      u64,      // 0 = immortal
    pub silo_id:          u64,
    pub flags:            u32,
    /// HMAC-SHA-256(silo_cap_key, cap_type || oid_prefix || expiry || silo_id || flags)
    pub signature:        [u8; 32],
}

impl CapToken {
    /// Produce the 40-byte signing payload.
    fn signing_payload(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&(self.cap_type as u32).to_le_bytes());
        buf[4..12].copy_from_slice(&self.object_oid_prefix.to_le_bytes());
        buf[12..20].copy_from_slice(&self.expiry_tick.to_le_bytes());
        buf[20..28].copy_from_slice(&self.silo_id.to_le_bytes());
        buf[28..32].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    /// Verify this token against the expected signing key.
    pub fn verify(&self, signing_key: &[u8; 32], now_tick: u64) -> bool {
        // Check expiry
        if self.expiry_tick != 0 && now_tick > self.expiry_tick { return false; }
        // Verify HMAC
        let payload = self.signing_payload();
        let expected = hmac_sha256(signing_key, &payload);
        self.signature == expected
    }

    pub fn has_flag(&self, flag: u32) -> bool { self.flags & flag != 0 }
}

// ── CapToken Forge ────────────────────────────────────────────────────────────

/// Per-Silo token set.
#[derive(Debug, Default, Clone)]
pub struct SiloCapSet {
    pub tokens: Vec<CapToken>,
}

impl SiloCapSet {
    pub fn has(&self, cap: CapType, flag: u32, oid_prefix: u64, now_tick: u64, signing_key: &[u8; 32]) -> bool {
        self.tokens.iter().any(|t|
            t.cap_type == cap &&
            t.has_flag(flag) &&
            (t.object_oid_prefix == 0 || t.object_oid_prefix == oid_prefix) &&
            t.verify(signing_key, now_tick)
        )
    }
    pub fn revoke_all(&mut self) { self.tokens.clear(); }
}

// ── Forge Statistics ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ForgeStats {
    pub minted: u64,
    pub revoked: u64,
    pub checks_ok: u64,
    pub checks_denied: u64,
    pub expired: u64,
}

// ── CapToken Forge ────────────────────────────────────────────────────────────

/// Mints, grants, verifies, and revokes CapTokens for all Silos.
pub struct CapTokenForge {
    /// Per-Silo cap sets
    silo_caps: BTreeMap<u64, SiloCapSet>,
    /// Per-Silo signing keys (derived from IdentityTpmBridge::silo_cap_key)
    silo_keys: BTreeMap<u64, [u8; 32]>,
    pub stats: ForgeStats,
}

impl CapTokenForge {
    pub fn new() -> Self {
        CapTokenForge {
            silo_caps: BTreeMap::new(),
            silo_keys: BTreeMap::new(),
            stats: ForgeStats::default(),
        }
    }

    /// Register a new Silo with its derived signing key.
    /// Called from boot_sequence::on_silo_ready().
    pub fn register_silo(&mut self, silo_id: u64, signing_key: [u8; 32]) {
        self.silo_caps.insert(silo_id, SiloCapSet::default());
        self.silo_keys.insert(silo_id, signing_key);
    }

    /// Mint a new CapToken and add it to the Silo's cap set.
    pub fn mint(
        &mut self,
        silo_id: u64,
        cap_type: CapType,
        oid_prefix: u64,
        expiry_tick: u64,
        flags: u32,
    ) -> Option<CapToken> {
        let key = self.silo_keys.get(&silo_id)?;
        let mut token = CapToken {
            cap_type, object_oid_prefix: oid_prefix,
            expiry_tick, silo_id, flags,
            signature: [0u8; 32],
        };
        let payload = token.signing_payload();
        token.signature = hmac_sha256(key, &payload);

        self.silo_caps.entry(silo_id).or_default().tokens.push(token.clone());
        self.stats.minted += 1;
        crate::serial_println!(
            "[CAP FORGE] Minted {:?} cap for Silo {} flags={:#x}", cap_type, silo_id, flags
        );
        Some(token)
    }

    /// Check if a Silo holds a valid CapToken for the requested operation.
    /// Called on every syscall that needs a capability check.
    pub fn check(
        &mut self,
        silo_id: u64,
        cap: CapType,
        flag: u32,
        oid_prefix: u64,
        now_tick: u64,
    ) -> bool {
        let key = match self.silo_keys.get(&silo_id) { Some(k) => *k, None => return false };
        let set = match self.silo_caps.get(&silo_id) { Some(s) => s, None => return false };
        let ok = set.has(cap, flag, oid_prefix, now_tick, &key);
        if ok { self.stats.checks_ok += 1; } else { self.stats.checks_denied += 1; }
        ok
    }

    /// Revoke all tokens for a Silo (on vaporize).
    pub fn revoke_silo(&mut self, silo_id: u64) {
        if let Some(set) = self.silo_caps.get_mut(&silo_id) {
            self.stats.revoked += set.tokens.len() as u64;
            set.revoke_all();
        }
        self.silo_caps.remove(&silo_id);
        self.silo_keys.remove(&silo_id);
    }

    /// Grant baseline caps to a newly spawned Silo.
    /// All Silos get: Prism(READ), Aether(READ|WRITE), IPC(READ|WRITE), UI.
    pub fn grant_baseline(&mut self, silo_id: u64, now_tick: u64) {
        let expiry = now_tick + 1_000_000; // ~1M ticks = ~16 minutes
        self.mint(silo_id, CapType::Prism,   0, expiry, CAP_READ);
        self.mint(silo_id, CapType::Aether,  0, expiry, CAP_READ | CAP_WRITE);
        self.mint(silo_id, CapType::Ipc,     0, expiry, CAP_READ | CAP_WRITE);
        self.mint(silo_id, CapType::Energy,  0, expiry, CAP_READ);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  CapTokenForge: minted={} revoked={} ok={} denied={} silos={}",
            self.stats.minted, self.stats.revoked,
            self.stats.checks_ok, self.stats.checks_denied,
            self.silo_caps.len()
        );
    }
}
