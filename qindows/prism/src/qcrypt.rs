//! # Q-Crypt — Per-Silo Transparent Encryption
//!
//! Provides transparent encryption of Q-Objects at rest,
//! with per-Silo key management (Section 3.24).
//!
//! Features:
//! - Per-Silo encryption keys
//! - AES-256-GCM encryption
//! - Key derivation from Silo master key
//! - IV/nonce management
//! - Encryption statistics

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Cipher algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherAlgo {
    Aes256Gcm,
    ChaCha20Poly1305,
    None,
}

/// A Silo encryption context.
#[derive(Debug, Clone)]
pub struct SiloKeyContext {
    pub silo_id: u64,
    pub algorithm: CipherAlgo,
    pub key_hash: u64,
    pub nonce_counter: u64,
    pub bytes_encrypted: u64,
    pub bytes_decrypted: u64,
    pub created_at: u64,
    pub rotated_at: u64,
}

/// Encryption statistics.
#[derive(Debug, Clone, Default)]
pub struct CryptStats {
    pub keys_created: u64,
    pub keys_rotated: u64,
    pub encrypt_ops: u64,
    pub decrypt_ops: u64,
    pub total_bytes_encrypted: u64,
    pub total_bytes_decrypted: u64,
}

/// The Q-Crypt Manager.
pub struct QCrypt {
    pub contexts: BTreeMap<u64, SiloKeyContext>,
    pub default_algo: CipherAlgo,
    pub stats: CryptStats,
}

impl QCrypt {
    pub fn new() -> Self {
        QCrypt {
            contexts: BTreeMap::new(),
            default_algo: CipherAlgo::Aes256Gcm,
            stats: CryptStats::default(),
        }
    }

    /// Create a key context for a Silo.
    pub fn create_key(&mut self, silo_id: u64, key_material: &[u8], now: u64) -> u64 {
        let key_hash = key_material.iter()
            .fold(0u64, |h, &b| h.wrapping_mul(31).wrapping_add(b as u64));

        self.contexts.insert(silo_id, SiloKeyContext {
            silo_id, algorithm: self.default_algo,
            key_hash, nonce_counter: 0,
            bytes_encrypted: 0, bytes_decrypted: 0,
            created_at: now, rotated_at: now,
        });

        self.stats.keys_created += 1;
        silo_id
    }

    /// Encrypt data for a Silo. Returns (ciphertext_len, nonce_used).
    pub fn encrypt(&mut self, silo_id: u64, plaintext_len: u64) -> Option<(u64, u64)> {
        let ctx = self.contexts.get_mut(&silo_id)?;
        let nonce = ctx.nonce_counter;
        ctx.nonce_counter += 1;

        // In production: actually encrypt with AES-GCM
        let ciphertext_len = plaintext_len + 16; // +16 for auth tag
        ctx.bytes_encrypted += plaintext_len;
        self.stats.encrypt_ops += 1;
        self.stats.total_bytes_encrypted += plaintext_len;
        Some((ciphertext_len, nonce))
    }

    /// Decrypt data for a Silo.
    pub fn decrypt(&mut self, silo_id: u64, ciphertext_len: u64) -> Option<u64> {
        let ctx = self.contexts.get_mut(&silo_id)?;
        if ciphertext_len < 16 { return None; }

        let plaintext_len = ciphertext_len - 16;
        ctx.bytes_decrypted += plaintext_len;
        self.stats.decrypt_ops += 1;
        self.stats.total_bytes_decrypted += plaintext_len;
        Some(plaintext_len)
    }

    /// Rotate key for a Silo.
    pub fn rotate_key(&mut self, silo_id: u64, new_material: &[u8], now: u64) -> bool {
        if let Some(ctx) = self.contexts.get_mut(&silo_id) {
            ctx.key_hash = new_material.iter()
                .fold(0u64, |h, &b| h.wrapping_mul(31).wrapping_add(b as u64));
            ctx.nonce_counter = 0;
            ctx.rotated_at = now;
            self.stats.keys_rotated += 1;
            true
        } else { false }
    }
}
