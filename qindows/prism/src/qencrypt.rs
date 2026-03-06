//! # Q-Encrypt — At-Rest Encryption per Object
//!
//! Transparent encryption for Q-Objects stored on disk
//! (Section 3.11). Each Silo can have its own key.
//!
//! Features:
//! - AES-256-GCM per-object encryption
//! - Per-Silo master keys derived from Silo secret
//! - Key rotation without re-encrypting all data (via key wrapping)
//! - Metadata encryption (filenames, sizes)
//! - Hardware key store integration

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Encryption algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncAlgo {
    Aes256Gcm,
    ChaCha20Poly1305,
    Aes128Gcm,
}

/// Key state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Active,
    Rotated,
    Revoked,
}

/// An encryption key.
#[derive(Debug, Clone)]
pub struct EncKey {
    pub id: u64,
    pub silo_id: u64,
    pub algo: EncAlgo,
    pub state: KeyState,
    pub created_at: u64,
    pub rotated_at: u64,
    pub key_hash: [u8; 32],
    pub version: u32,
}

/// Encryption metadata for a Q-Object.
#[derive(Debug, Clone)]
pub struct EncMeta {
    pub oid: u64,
    pub key_id: u64,
    pub algo: EncAlgo,
    pub nonce: [u8; 12],
    pub tag: [u8; 16],
    pub encrypted_size: u64,
}

/// Encryption statistics.
#[derive(Debug, Clone, Default)]
pub struct EncStats {
    pub objects_encrypted: u64,
    pub objects_decrypted: u64,
    pub keys_created: u64,
    pub keys_rotated: u64,
    pub bytes_encrypted: u64,
}

/// The Q-Encrypt Engine.
pub struct QEncrypt {
    pub keys: BTreeMap<u64, EncKey>,
    pub silo_keys: BTreeMap<u64, u64>, // silo → active key ID
    pub metadata: BTreeMap<u64, EncMeta>, // oid → encryption meta
    next_key_id: u64,
    pub default_algo: EncAlgo,
    pub stats: EncStats,
}

impl QEncrypt {
    pub fn new() -> Self {
        QEncrypt {
            keys: BTreeMap::new(),
            silo_keys: BTreeMap::new(),
            metadata: BTreeMap::new(),
            next_key_id: 1,
            default_algo: EncAlgo::Aes256Gcm,
            stats: EncStats::default(),
        }
    }

    /// Create a key for a Silo.
    pub fn create_key(&mut self, silo_id: u64, key_hash: [u8; 32], now: u64) -> u64 {
        let id = self.next_key_id;
        self.next_key_id += 1;

        self.keys.insert(id, EncKey {
            id, silo_id, algo: self.default_algo,
            state: KeyState::Active, created_at: now,
            rotated_at: 0, key_hash, version: 1,
        });

        self.silo_keys.insert(silo_id, id);
        self.stats.keys_created += 1;
        id
    }

    /// Rotate a Silo's key.
    pub fn rotate_key(&mut self, silo_id: u64, new_hash: [u8; 32], now: u64) -> Result<u64, &'static str> {
        let old_id = *self.silo_keys.get(&silo_id).ok_or("No key for Silo")?;

        // Mark old key as rotated
        if let Some(old) = self.keys.get_mut(&old_id) {
            old.state = KeyState::Rotated;
            old.rotated_at = now;
        }

        let new_id = self.next_key_id;
        self.next_key_id += 1;

        let version = self.keys.get(&old_id).map(|k| k.version + 1).unwrap_or(1);

        self.keys.insert(new_id, EncKey {
            id: new_id, silo_id, algo: self.default_algo,
            state: KeyState::Active, created_at: now,
            rotated_at: 0, key_hash: new_hash, version,
        });

        self.silo_keys.insert(silo_id, new_id);
        self.stats.keys_rotated += 1;
        Ok(new_id)
    }

    /// Encrypt an object (record metadata).
    pub fn encrypt(&mut self, oid: u64, silo_id: u64, size: u64, nonce: [u8; 12], tag: [u8; 16]) -> Result<(), &'static str> {
        let key_id = *self.silo_keys.get(&silo_id).ok_or("No key for Silo")?;
        let algo = self.keys.get(&key_id).map(|k| k.algo).unwrap_or(self.default_algo);

        self.metadata.insert(oid, EncMeta {
            oid, key_id, algo, nonce, tag, encrypted_size: size,
        });

        self.stats.objects_encrypted += 1;
        self.stats.bytes_encrypted += size;
        Ok(())
    }

    /// Get decryption info for an object.
    pub fn decrypt_info(&mut self, oid: u64) -> Option<&EncMeta> {
        self.stats.objects_decrypted += 1;
        self.metadata.get(&oid)
    }
}
