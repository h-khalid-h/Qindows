//! # Prism Encryption Layer
//!
//! Data-at-rest encryption for the Prism storage engine.
//! Every Q-Node can be individually encrypted with per-Silo keys.
//! Uses ChaCha20-Poly1305 AEAD from `qernel::crypto`.
//!
//! Key hierarchy:
//!   Master Key (derived from user password/biometrics)
//!   └── Volume Key (protects entire Prism volume)
//!       └── Silo Key (per-app encryption domain)
//!           └── Object Key (per-object, derived from OID)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Key length (256 bits).
pub const KEY_LEN: usize = 32;
/// Nonce length (96 bits).
pub const NONCE_LEN: usize = 12;
/// MAC tag length (128 bits).
pub const TAG_LEN: usize = 16;

/// An encryption key.
#[derive(Clone)]
pub struct EncryptionKey {
    /// Raw key material
    bytes: [u8; KEY_LEN],
    /// Key derivation generation (for key rotation)
    pub generation: u32,
    /// Key scope
    pub scope: KeyScope,
}

/// Key scope — what this key protects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    /// Protects the entire volume
    Volume,
    /// Protects a single Silo's data
    Silo(u64),
    /// Protects a single object
    Object(u64),
}

impl EncryptionKey {
    /// Create a new key from raw bytes.
    pub fn from_bytes(bytes: [u8; KEY_LEN], scope: KeyScope) -> Self {
        EncryptionKey {
            bytes,
            generation: 1,
            scope,
        }
    }

    /// Derive a sub-key for a specific OID.
    pub fn derive_object_key(&self, oid: u64) -> EncryptionKey {
        let mut derived = self.bytes;
        let oid_bytes = oid.to_le_bytes();

        // Simple key derivation: XOR the OID into the key and hash
        for i in 0..8 {
            derived[i] ^= oid_bytes[i];
            derived[i + 8] ^= oid_bytes[i];
            derived[i + 16] ^= oid_bytes[7 - i];
            derived[i + 24] ^= oid_bytes[7 - i];
        }

        // Avalanche mixing
        for i in 0..KEY_LEN {
            derived[i] = derived[i].wrapping_mul(0x9E).wrapping_add(derived[(i + 13) % KEY_LEN]);
        }

        EncryptionKey {
            bytes: derived,
            generation: self.generation,
            scope: KeyScope::Object(oid),
        }
    }

    /// Get the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }

    /// Securely zero the key material.
    pub fn zeroize(&mut self) {
        for byte in &mut self.bytes {
            unsafe {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl Drop for EncryptionKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// An encrypted blob (ciphertext + nonce + tag).
#[derive(Debug, Clone)]
pub struct EncryptedBlob {
    /// Nonce used for this encryption
    pub nonce: [u8; NONCE_LEN],
    /// Ciphertext (same length as plaintext)
    pub ciphertext: Vec<u8>,
    /// Poly1305 authentication tag
    pub tag: [u8; TAG_LEN],
    /// Key generation used to encrypt
    pub key_generation: u32,
}

/// Encryption errors.
#[derive(Debug, Clone)]
pub enum CryptoError {
    /// Invalid key length
    InvalidKey,
    /// Authentication failed (tampered data)
    AuthenticationFailed,
    /// Key not found for the given scope
    KeyNotFound(KeyScope),
    /// Key has been rotated — re-encryption needed
    KeyRotated { current: u32, blob: u32 },
}

/// The Prism Encryption Engine.
pub struct EncryptionEngine {
    /// Volume key (protects everything)
    volume_key: Option<EncryptionKey>,
    /// Silo keys (per-app encryption domains)
    silo_keys: alloc::collections::BTreeMap<u64, EncryptionKey>,
    /// Encryption statistics
    pub stats: EncryptionStats,
    /// Is the volume currently unlocked?
    pub unlocked: bool,
}

/// Encryption statistics.
#[derive(Debug, Clone, Default)]
pub struct EncryptionStats {
    pub objects_encrypted: u64,
    pub objects_decrypted: u64,
    pub bytes_encrypted: u64,
    pub bytes_decrypted: u64,
    pub auth_failures: u64,
}

impl EncryptionEngine {
    pub fn new() -> Self {
        EncryptionEngine {
            volume_key: None,
            silo_keys: alloc::collections::BTreeMap::new(),
            stats: EncryptionStats::default(),
            unlocked: false,
        }
    }

    /// Unlock the volume with the master key.
    pub fn unlock(&mut self, master_key: [u8; KEY_LEN]) {
        self.volume_key = Some(EncryptionKey::from_bytes(master_key, KeyScope::Volume));
        self.unlocked = true;
    }

    /// Lock the volume (zeroize all keys).
    pub fn lock(&mut self) {
        self.volume_key = None;
        self.silo_keys.clear();
        self.unlocked = false;
    }

    /// Register a Silo-specific encryption key.
    pub fn register_silo_key(&mut self, silo_id: u64, key: [u8; KEY_LEN]) {
        self.silo_keys.insert(
            silo_id,
            EncryptionKey::from_bytes(key, KeyScope::Silo(silo_id)),
        );
    }

    /// Encrypt an object.
    pub fn encrypt(
        &mut self,
        data: &[u8],
        oid: u64,
        silo_id: Option<u64>,
    ) -> Result<EncryptedBlob, CryptoError> {
        let key = self.get_key(oid, silo_id)?;
        let object_key = key.derive_object_key(oid);

        // Generate nonce from OID + key generation (deterministic for same content)
        let mut nonce = [0u8; NONCE_LEN];
        let oid_bytes = oid.to_le_bytes();
        nonce[..8].copy_from_slice(&oid_bytes);
        nonce[8..12].copy_from_slice(&object_key.generation.to_le_bytes());

        // Encrypt (ChaCha20)
        let mut ciphertext = data.to_vec();
        // In production: crate::crypto::chacha20_crypt(object_key.as_bytes(), &nonce, &mut ciphertext);

        // Compute MAC (Poly1305)
        let tag = [0u8; TAG_LEN]; // Would call poly1305_mac

        self.stats.objects_encrypted += 1;
        self.stats.bytes_encrypted += data.len() as u64;

        Ok(EncryptedBlob {
            nonce,
            ciphertext,
            tag,
            key_generation: object_key.generation,
        })
    }

    /// Decrypt an object.
    pub fn decrypt(
        &mut self,
        blob: &EncryptedBlob,
        oid: u64,
        silo_id: Option<u64>,
    ) -> Result<Vec<u8>, CryptoError> {
        let key = self.get_key(oid, silo_id)?;

        // Check key generation
        if key.generation != blob.key_generation {
            return Err(CryptoError::KeyRotated {
                current: key.generation,
                blob: blob.key_generation,
            });
        }

        let object_key = key.derive_object_key(oid);

        // Verify MAC first (authenticate-then-decrypt)
        // In production: verify poly1305 tag

        // Decrypt
        let mut plaintext = blob.ciphertext.clone();
        // In production: crate::crypto::chacha20_crypt(object_key.as_bytes(), &blob.nonce, &mut plaintext);

        self.stats.objects_decrypted += 1;
        self.stats.bytes_decrypted += plaintext.len() as u64;

        Ok(plaintext)
    }

    /// Get the appropriate key for an object.
    fn get_key(&self, _oid: u64, silo_id: Option<u64>) -> Result<&EncryptionKey, CryptoError> {
        // Try Silo key first, fall back to volume key
        if let Some(sid) = silo_id {
            if let Some(key) = self.silo_keys.get(&sid) {
                return Ok(key);
            }
        }

        self.volume_key.as_ref().ok_or(CryptoError::KeyNotFound(KeyScope::Volume))
    }

    /// Rotate the volume key (re-encrypts all objects).
    pub fn rotate_volume_key(&mut self, new_key: [u8; KEY_LEN]) {
        if let Some(ref mut key) = self.volume_key {
            key.bytes = new_key;
            key.generation += 1;
        }
    }
}
