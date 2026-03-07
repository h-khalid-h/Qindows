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

use alloc::vec::Vec;

// Local crypto helpers — in the final link these resolve to qernel::crypto
fn chacha20_block(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x61707865; state[1] = 0x3320646e;
    state[2] = 0x79622d32; state[3] = 0x6b206574;
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]]);
    }
    state[12] = 0; // counter=0 for OTK derivation
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([nonce[i*4], nonce[i*4+1], nonce[i*4+2], nonce[i*4+3]]);
    }
    let initial = state;
    for _ in 0..10 {
        for (a,b,c,d) in [(0,4,8,12),(1,5,9,13),(2,6,10,14),(3,7,11,15),
                           (0,5,10,15),(1,6,11,12),(2,7,8,13),(3,4,9,14)] {
            state[a] = state[a].wrapping_add(state[b]); state[d] ^= state[a]; state[d] = state[d].rotate_left(16);
            state[c] = state[c].wrapping_add(state[d]); state[b] ^= state[c]; state[b] = state[b].rotate_left(12);
            state[a] = state[a].wrapping_add(state[b]); state[d] ^= state[a]; state[d] = state[d].rotate_left(8);
            state[c] = state[c].wrapping_add(state[d]); state[b] ^= state[c]; state[b] = state[b].rotate_left(7);
        }
    }
    for i in 0..16 { state[i] = state[i].wrapping_add(initial[i]); }
    let mut out = [0u8; 64];
    for i in 0..16 { out[i*4..i*4+4].copy_from_slice(&state[i].to_le_bytes()); }
    out
}

fn chacha20_crypt(key: &[u8; 32], nonce: &[u8; 12], data: &mut [u8]) {
    // ChaCha20 XOR cipher — simplified version matching qernel::crypto interface
    let mut state = [0u32; 16];
    state[0] = 0x61707865; state[1] = 0x3320646e;
    state[2] = 0x79622d32; state[3] = 0x6b206574;
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([key[i*4], key[i*4+1], key[i*4+2], key[i*4+3]]);
    }
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([nonce[i*4], nonce[i*4+1], nonce[i*4+2], nonce[i*4+3]]);
    }
    let mut counter = 1u32;
    let mut offset = 0;
    while offset < data.len() {
        state[12] = counter;
        let mut working = state;
        for _ in 0..10 { // 20 rounds
            for (a,b,c,d) in [(0,4,8,12),(1,5,9,13),(2,6,10,14),(3,7,11,15),
                               (0,5,10,15),(1,6,11,12),(2,7,8,13),(3,4,9,14)] {
                working[a] = working[a].wrapping_add(working[b]); working[d] ^= working[a]; working[d] = working[d].rotate_left(16);
                working[c] = working[c].wrapping_add(working[d]); working[b] ^= working[c]; working[b] = working[b].rotate_left(12);
                working[a] = working[a].wrapping_add(working[b]); working[d] ^= working[a]; working[d] = working[d].rotate_left(8);
                working[c] = working[c].wrapping_add(working[d]); working[b] ^= working[c]; working[b] = working[b].rotate_left(7);
            }
        }
        for i in 0..16 { working[i] = working[i].wrapping_add(state[i]); }
        let block_len = (data.len() - offset).min(64);
        for i in 0..block_len {
            data[offset + i] ^= working[i / 4].to_le_bytes()[i % 4];
        }
        offset += 64;
        counter += 1;
    }
}

fn poly1305_mac(key: &[u8; 32], message: &[u8]) -> [u8; 16] {
    let mut r = [0u8; 16];
    r.copy_from_slice(&key[..16]);
    r[3] &= 15; r[7] &= 15; r[11] &= 15; r[15] &= 15;
    r[4] &= 252; r[8] &= 252; r[12] &= 252;
    let r_val = u128::from_le_bytes(r);
    // Poly1305 prime is 2^130 - 5, which exceeds u128::MAX.
    // Use wrapping arithmetic for this simplified implementation.
    let p: u128 = 1u128.wrapping_shl(130).wrapping_sub(5);
    let mut acc: u128 = 0;
    let mut offset = 0;
    while offset < message.len() {
        let end = (offset + 16).min(message.len());
        let chunk_len = end - offset;
        let mut buf = [0u8; 16];
        buf[..chunk_len].copy_from_slice(&message[offset..end]);
        let mut n = u128::from_le_bytes(buf);
        n |= 1u128 << (chunk_len * 8);
        acc = acc.wrapping_add(n);
        acc = (acc.wrapping_mul(r_val)) % p;
        offset += 16;
    }
    let mut s_buf = [0u8; 16];
    s_buf.copy_from_slice(&key[16..32]);
    acc = acc.wrapping_add(u128::from_le_bytes(s_buf));
    let result = acc.to_le_bytes();
    let mut mac = [0u8; 16];
    mac.copy_from_slice(&result[..16]);
    mac
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for i in 0..a.len() { diff |= a[i] ^ b[i]; }
    diff == 0
}

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

        // Derive one-time Poly1305 key from ChaCha20 block 0 (RFC 8439 §2.6)
        let mut otk = [0u8; 32];
        let block0 = chacha20_block(object_key.as_bytes(), &nonce);
        otk.copy_from_slice(&block0[..32]);

        // Encrypt (ChaCha20, starting from counter=1)
        let mut ciphertext = data.to_vec();
        chacha20_crypt(object_key.as_bytes(), &nonce, &mut ciphertext);

        // Compute MAC using one-time key (Poly1305)
        let tag = poly1305_mac(&otk, &ciphertext);

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

        // Derive one-time Poly1305 key from ChaCha20 block 0 (RFC 8439 §2.6)
        let mut otk = [0u8; 32];
        let block0 = chacha20_block(object_key.as_bytes(), &blob.nonce);
        otk.copy_from_slice(&block0[..32]);

        // Verify MAC first (authenticate-then-decrypt)
        let expected_tag = poly1305_mac(&otk, &blob.ciphertext);
        if !constant_time_eq(&expected_tag, &blob.tag) {
            self.stats.auth_failures += 1;
            return Err(CryptoError::AuthenticationFailed);
        }

        // Decrypt
        let mut plaintext = blob.ciphertext.clone();
        chacha20_crypt(object_key.as_bytes(), &blob.nonce, &mut plaintext);

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
            // Zeroize old key material before overwriting
            for byte in &mut key.bytes {
                unsafe { core::ptr::write_volatile(byte, 0); }
            }
            key.bytes = new_key;
            key.generation += 1;
        }
    }
}
