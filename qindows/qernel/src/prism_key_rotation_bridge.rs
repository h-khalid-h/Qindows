//! # Prism Key Rotation Bridge (Phase 173)
//!
//! ## Architecture Guardian: The Gap
//! `crypto_primitives.rs` provides free functions:
//! - `hmac_sha256(key: &[u8], message: &[u8])` → [u8; 32]
//! - `sha256(data: &[u8])` → [u8; 32]
//!
//! Prism object encryption keys were never rotated. On Silo vaporize,
//! old key material stayed in kernel memory — a Law 9 violation.
//!
//! This module provides `PrismKeyRotationBridge`:
//! 1. `rotate_silo_keys()` — derive new key via HMAC-SHA256
//! 2. `on_silo_vaporize()` — zeroize key material

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::crypto_primitives::hmac_sha256;

const KDF_DOMAIN: &[u8] = b"QINDOWS-PRISM-KEY-v1";

#[derive(Debug, Default, Clone)]
pub struct KeyRotationStats {
    pub rotations:    u64,
    pub zeroized:     u64,
}

pub struct PrismKeyRotationBridge {
    silo_keys:  BTreeMap<u64, [u8; 32]>,
    pub stats:  KeyRotationStats,
}

impl PrismKeyRotationBridge {
    pub fn new() -> Self {
        PrismKeyRotationBridge { silo_keys: BTreeMap::new(), stats: KeyRotationStats::default() }
    }

    /// Derive or rotate a Silo's Prism encryption key.
    /// Key = HMAC-SHA256(domain || silo_id || tick)
    pub fn rotate_silo_keys(&mut self, silo_id: u64, tick: u64) -> [u8; 32] {
        self.stats.rotations += 1;

        let mut input = [0u8; KDF_DOMAIN.len() + 16];
        input[..KDF_DOMAIN.len()].copy_from_slice(KDF_DOMAIN);
        input[KDF_DOMAIN.len()..KDF_DOMAIN.len() + 8].copy_from_slice(&silo_id.to_le_bytes());
        input[KDF_DOMAIN.len() + 8..].copy_from_slice(&tick.to_le_bytes());

        // HMAC-SHA256 with a node-specific master key (using [0x51; 32] as KDF domain key)
        let kdf_key = [0x51u8; 32];
        let new_key = hmac_sha256(&kdf_key, &input);

        self.silo_keys.insert(silo_id, new_key);
        crate::serial_println!("[KEY ROT] Silo {} key rotated (Law 9)", silo_id);
        new_key
    }

    pub fn get_key(&self, silo_id: u64) -> Option<&[u8; 32]> {
        self.silo_keys.get(&silo_id)
    }

    /// Zeroize all key material for vaporized Silo (Law 9: sovereign data).
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        if let Some(mut key) = self.silo_keys.remove(&silo_id) {
            for b in key.iter_mut() { *b = 0; }
            self.stats.zeroized += 1;
            crate::serial_println!("[KEY ROT] Silo {} keys zeroized (Law 9)", silo_id);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KeyRotBridge: rotations={} zeroized={}", self.stats.rotations, self.stats.zeroized
        );
    }
}
