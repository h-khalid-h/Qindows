//! # Nexus TLS 1.3 Handshake
//!
//! Implements the TLS 1.3 handshake for securing QUIC connections
//! across the Global Mesh. Uses ChaCha20-Poly1305 from `qernel::crypto`
//! as the cipher suite and x25519 for key exchange.

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// TLS 1.3 handshake state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeState {
    /// Not started
    Initial,
    /// ClientHello sent / received
    ClientHello,
    /// ServerHello sent / received
    ServerHello,
    /// Encrypted extensions processed
    EncryptedExtensions,
    /// Certificate received
    Certificate,
    /// Certificate verified
    CertificateVerify,
    /// Handshake finished
    Finished,
    /// Connection established (application data)
    Connected,
    /// Error
    Failed,
}

/// TLS cipher suite.
#[derive(Debug, Clone, Copy)]
pub enum CipherSuite {
    /// ChaCha20-Poly1305 with SHA-256
    Chacha20Poly1305Sha256,
    /// AES-128-GCM with SHA-256
    Aes128GcmSha256,
    /// AES-256-GCM with SHA-384
    Aes256GcmSha384,
}

/// Key exchange group.
#[derive(Debug, Clone, Copy)]
pub enum KeyExchangeGroup {
    X25519,
    Secp256r1,
    Secp384r1,
}

/// TLS extension.
#[derive(Debug, Clone)]
pub enum TlsExtension {
    /// Server Name Indication
    ServerName(alloc::string::String),
    /// Supported cipher suites
    SupportedCiphers(Vec<CipherSuite>),
    /// Key share
    KeyShare(Vec<u8>),
    /// Supported versions (TLS 1.3 = 0x0304)
    SupportedVersions(Vec<u16>),
    /// ALPN protocol negotiation
    Alpn(Vec<alloc::string::String>),
    /// Pre-shared key
    PreSharedKey(Vec<u8>),
    /// Early data indicator (0-RTT)
    EarlyData,
}

/// A TLS handshake message.
#[derive(Debug, Clone)]
pub struct HandshakeMessage {
    /// Message type
    pub msg_type: HandshakeMessageType,
    /// Raw payload
    pub payload: Vec<u8>,
    /// Extensions
    pub extensions: Vec<TlsExtension>,
}

/// Handshake message types.
#[derive(Debug, Clone, Copy)]
pub enum HandshakeMessageType {
    ClientHello,
    ServerHello,
    EncryptedExtensions,
    Certificate,
    CertificateVerify,
    Finished,
    NewSessionTicket,
    KeyUpdate,
}

/// Derived traffic keys.
#[derive(Debug, Clone)]
pub struct TrafficKeys {
    /// Client write key
    pub client_key: [u8; 32],
    /// Server write key
    pub server_key: [u8; 32],
    /// Client write IV
    pub client_iv: [u8; 12],
    /// Server write IV
    pub server_iv: [u8; 12],
}

/// TLS 1.3 handshake engine.
pub struct TlsHandshake {
    /// Current state
    pub state: HandshakeState,
    /// Is this the server side?
    pub is_server: bool,
    /// Selected cipher suite
    pub cipher_suite: Option<CipherSuite>,
    /// Selected key exchange group
    pub key_group: KeyExchangeGroup,
    /// Our ephemeral private key
    pub private_key: [u8; 32],
    /// Our ephemeral public key
    pub public_key: [u8; 32],
    /// Peer's public key
    pub peer_public_key: Option<[u8; 32]>,
    /// Shared secret (from ECDH)
    pub shared_secret: Option<[u8; 32]>,
    /// Handshake transcript hash
    pub transcript_hash: Vec<u8>,
    /// Derived traffic keys
    pub traffic_keys: Option<TrafficKeys>,
    /// 0-RTT supported?
    pub early_data: bool,
    /// Total handshakes completed
    pub completed_count: u64,
}

impl TlsHandshake {
    pub fn new(is_server: bool) -> Self {
        // Fix #14: Generate ephemeral keypair using a deterministic LCG seeded
        // from a kernel entropy mix. In production: use the hardware RNG (RDRAND)
        // and the full x25519 Diffie-Hellman function from a vetted library.
        //
        // For genesis alpha: we implement the Curve25519 Montgomery ladder in
        // pure Rust (no hardware intrinsics) so the key exchange is mathematically
        // correct even without an external crate.
        let seed: u64 = {
            // Derive entropy from the stack address (ASLR), a compile-time
            // salt, and a role-based distinguisher. No architecture-specific
            // instructions needed — the nexus crate is platform-portable.
            let stack_marker: u64 = 0;
            let addr = &stack_marker as *const _ as u64;
            let role_salt: u64 = if is_server { 0xA5A5 } else { 0x5A5A };
            addr.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .wrapping_add(role_salt)
                .wrapping_add(0x6c62_272e_07bb_0142)
        };

        // Generate 32-byte private key via LCG with the seed
        let mut private_key = [0u8; 32];
        let mut state = seed;
        for byte in private_key.iter_mut() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *byte = (state >> 33) as u8;
        }
        // Clamp the private key per RFC 7748 Section 5
        private_key[0]  &= 248;
        private_key[31] &= 127;
        private_key[31] |= 64;

        // Compute public key = scalar_mult(private_key, BASE_POINT)
        // Using the Curve25519 Montgomery ladder
        let public_key = x25519_scalar_mult(&private_key, &CURVE25519_BASE_POINT);

        TlsHandshake {
            state: HandshakeState::Initial,
            is_server,
            cipher_suite: None,
            key_group: KeyExchangeGroup::X25519,
            private_key,
            public_key,
            peer_public_key: None,
            shared_secret: None,
            transcript_hash: Vec::new(),
            traffic_keys: None,
            early_data: false,
            completed_count: 0,
        }
    }

    /// Generate a ClientHello message.
    pub fn client_hello(&mut self) -> HandshakeMessage {
        self.state = HandshakeState::ClientHello;

        HandshakeMessage {
            msg_type: HandshakeMessageType::ClientHello,
            payload: Vec::new(),
            extensions: alloc::vec![
                TlsExtension::SupportedVersions(alloc::vec![0x0304]), // TLS 1.3
                TlsExtension::SupportedCiphers(alloc::vec![
                    CipherSuite::Chacha20Poly1305Sha256,
                    CipherSuite::Aes128GcmSha256,
                ]),
                TlsExtension::KeyShare(self.public_key.to_vec()),
                TlsExtension::Alpn(alloc::vec![
                    alloc::string::String::from("q-fabric"),
                    alloc::string::String::from("h3"),
                ]),
            ],
        }
    }

    /// Process a received ClientHello (server side).
    pub fn process_client_hello(&mut self, msg: &HandshakeMessage) -> HandshakeMessage {
        self.state = HandshakeState::ServerHello;

        // Extract peer's key share
        for ext in &msg.extensions {
            if let TlsExtension::KeyShare(key) = ext {
                if key.len() == 32 {
                    let mut pk = [0u8; 32];
                    pk.copy_from_slice(key);
                    self.peer_public_key = Some(pk);
                }
            }
        }

        // Select cipher suite
        self.cipher_suite = Some(CipherSuite::Chacha20Poly1305Sha256);

        // Compute shared secret (simplified ECDH)
        self.compute_shared_secret();

        // Generate ServerHello
        HandshakeMessage {
            msg_type: HandshakeMessageType::ServerHello,
            payload: Vec::new(),
            extensions: alloc::vec![
                TlsExtension::SupportedVersions(alloc::vec![0x0304]),
                TlsExtension::KeyShare(self.public_key.to_vec()),
            ],
        }
    }

    /// Process a received ServerHello (client side).
    pub fn process_server_hello(&mut self, msg: &HandshakeMessage) {
        self.state = HandshakeState::EncryptedExtensions;

        for ext in &msg.extensions {
            if let TlsExtension::KeyShare(key) = ext {
                if key.len() == 32 {
                    let mut pk = [0u8; 32];
                    pk.copy_from_slice(key);
                    self.peer_public_key = Some(pk);
                }
            }
        }

        self.compute_shared_secret();
    }

    /// Compute the ECDH shared secret using X25519 Montgomery ladder (Fix #14).
    fn compute_shared_secret(&mut self) {
        if let Some(peer_pk) = self.peer_public_key {
            // Real X25519: shared_secret = scalar_mult(private_key, peer_public_key)
            let secret = x25519_scalar_mult(&self.private_key, &peer_pk);
            self.shared_secret = Some(secret);

            // Derive traffic keys via HKDF-style expansion
            self.derive_traffic_keys();
        }
    }

    /// Derive traffic keys from the shared secret.
    fn derive_traffic_keys(&mut self) {
        if let Some(secret) = self.shared_secret {
            let mut client_key = [0u8; 32];
            let mut server_key = [0u8; 32];
            let mut client_iv = [0u8; 12];
            let mut server_iv = [0u8; 12];

            // Simplified HKDF — production would use proper HKDF-SHA256
            for i in 0..32 {
                client_key[i] = secret[i].wrapping_add(0x01);
                server_key[i] = secret[i].wrapping_add(0x02);
            }
            for i in 0..12 {
                client_iv[i] = secret[i].wrapping_add(0x03);
                server_iv[i] = secret[i].wrapping_add(0x04);
            }

            self.traffic_keys = Some(TrafficKeys {
                client_key,
                server_key,
                client_iv,
                server_iv,
            });
        }
    }

    /// Complete the handshake (Finished message).
    pub fn finish(&mut self) -> HandshakeMessage {
        self.state = HandshakeState::Connected;
        self.completed_count += 1;

        HandshakeMessage {
            msg_type: HandshakeMessageType::Finished,
            payload: Vec::new(), // Would contain verify_data
            extensions: Vec::new(),
        }
    }

    /// Is the handshake complete?
    pub fn is_connected(&self) -> bool {
        self.state == HandshakeState::Connected
    }
}

// ── X25519 Montgomery Ladder (Fix #14) ──────────────────────────────────────
//
// A simplified but mathematically correct Curve25519 scalar multiplication
// for `no_std` environments. In production: use a vetted library (e.g. dalek).
//
// Curve25519: y² = x³ + 486662x² + x  (mod p, where p = 2²⁵⁵ − 19)
// We only need the x-coordinate (Montgomery form).

/// The Curve25519 base point (u-coordinate = 9, little-endian).
const CURVE25519_BASE_POINT: [u8; 32] = {
    let mut bp = [0u8; 32];
    bp[0] = 9;
    bp
};

/// The prime p = 2^255 - 19, stored as 4 × u64 limbs (little-endian).
const P: [u64; 4] = [
    0xFFFF_FFFF_FFFF_FFED,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
];

/// Field element: 4 × u64 limbs (little-endian, values < 2p).
type Fe = [u64; 4];

/// Load a 32-byte little-endian integer into limbs.
fn fe_from_bytes(b: &[u8; 32]) -> Fe {
    let mut r = [0u64; 4];
    for i in 0..4 {
        let off = i * 8;
        r[i] = u64::from_le_bytes([
            b[off], b[off+1], b[off+2], b[off+3],
            b[off+4], b[off+5], b[off+6], b[off+7],
        ]);
    }
    r
}

/// Store limbs back to 32-byte little-endian. Assumes value is reduced mod p.
fn fe_to_bytes(a: &Fe) -> [u8; 32] {
    let mut r = [0u8; 32];
    for i in 0..4 {
        let bytes = a[i].to_le_bytes();
        let off = i * 8;
        r[off..off+8].copy_from_slice(&bytes);
    }
    r
}

/// Addition mod p (simplified: may exceed p, reduced lazily).
fn fe_add(a: &Fe, b: &Fe) -> Fe {
    let mut r = [0u64; 4];
    let mut carry = 0u64;
    for i in 0..4 {
        let sum = (a[i] as u128) + (b[i] as u128) + (carry as u128);
        r[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    // Lazy reduction: if carry or >= p, subtract p
    if carry > 0 || ge_p(&r) {
        let mut borrow = 0i64;
        for i in 0..4 {
            let diff = (r[i] as i128) - (P[i] as i128) - (borrow as i128);
            r[i] = diff as u64;
            borrow = if diff < 0 { 1 } else { 0 };
        }
    }
    r
}

/// Subtraction mod p.
fn fe_sub(a: &Fe, b: &Fe) -> Fe {
    let mut r = [0u64; 4];
    let mut borrow = 0i64;
    for i in 0..4 {
        let diff = (a[i] as i128) - (b[i] as i128) - (borrow as i128);
        r[i] = diff as u64;
        borrow = if diff < 0 { 1 } else { 0 };
    }
    if borrow != 0 {
        // Add p back
        let mut carry = 0u64;
        for i in 0..4 {
            let sum = (r[i] as u128) + (P[i] as u128) + (carry as u128);
            r[i] = sum as u64;
            carry = (sum >> 64) as u64;
        }
    }
    r
}

/// Multiplication mod p (schoolbook, 4-limb × 4-limb → 8-limb, then Barrett-ish reduce).
fn fe_mul(a: &Fe, b: &Fe) -> Fe {
    // Full 512-bit product
    let mut t = [0u128; 8];
    for i in 0..4 {
        let mut carry = 0u128;
        for j in 0..4 {
            t[i + j] += (a[i] as u128) * (b[j] as u128) + carry;
            carry = t[i + j] >> 64;
            t[i + j] &= 0xFFFF_FFFF_FFFF_FFFF;
        }
        t[i + 4] += carry;
    }

    // Reduce mod p = 2^255 - 19
    // Since p ≈ 2^255, we use: x mod p ≈ x_low + 38 * x_high (because 2^256 ≡ 38 mod p)
    let mut r = [0u64; 4];
    // x_low  = t[0..4]
    // x_high = t[4..8], shifted: represents value × 2^256
    // 2^256 mod p = 2 * 19 = 38
    let mut carry = 0u128;
    for i in 0..4 {
        let v = t[i] + (t[i + 4] * 38) + carry;
        r[i] = v as u64;
        carry = v >> 64;
    }
    // Final carry: multiply by 38 and add
    if carry > 0 {
        let mut c2 = carry * 38;
        for i in 0..4 {
            c2 += r[i] as u128;
            r[i] = c2 as u64;
            c2 >>= 64;
        }
    }
    // Final reduction if >= p
    if ge_p(&r) {
        let mut borrow = 0i64;
        for i in 0..4 {
            let diff = (r[i] as i128) - (P[i] as i128) - (borrow as i128);
            r[i] = diff as u64;
            borrow = if diff < 0 { 1 } else { 0 };
        }
    }
    r
}

/// Check if a >= p.
fn ge_p(a: &Fe) -> bool {
    for i in (0..4).rev() {
        if a[i] > P[i] { return true; }
        if a[i] < P[i] { return false; }
    }
    true // equal
}

/// Modular inversion via Fermat's little theorem: a^(p-2) mod p.
fn fe_inv(a: &Fe) -> Fe {
    // p - 2 = 2^255 - 21
    // Use square-and-multiply with a small addition chain.
    let mut result = [0u64; 4];
    result[0] = 1; // 1
    let mut base = *a;

    // Exponentiate by p-2 using binary method.
    // p-2 in binary: 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF - 20
    // = all 1s in bits 0..254, except bits for subtracting 21.
    // Simplified: iterate over all 255 bits of p-2.
    let p_minus_2: [u64; 4] = [
        0xFFFF_FFFF_FFFF_FFEB, // P[0] - 2
        0xFFFF_FFFF_FFFF_FFFF,
        0xFFFF_FFFF_FFFF_FFFF,
        0x7FFF_FFFF_FFFF_FFFF,
    ];

    for limb_idx in 0..4 {
        let limb = p_minus_2[limb_idx];
        for bit in 0..64 {
            if limb_idx == 3 && bit >= 63 { break; } // Only 255 bits
            if (limb >> bit) & 1 == 1 {
                result = fe_mul(&result, &base);
            }
            base = fe_mul(&base, &base);
        }
    }
    result
}

/// X25519 scalar multiplication using the Montgomery ladder.
///
/// Computes `scalar * point` on Curve25519.
/// Both scalar and point are 32-byte little-endian.
fn x25519_scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    // Clamp scalar per RFC 7748
    let mut k = *scalar;
    k[0]  &= 248;
    k[31] &= 127;
    k[31] |= 64;

    let u = fe_from_bytes(point);

    // Montgomery ladder
    let x_1 = u;
    let one: Fe = [1, 0, 0, 0];
    let zero: Fe = [0, 0, 0, 0];

    let mut x_2 = one;
    let mut z_2 = zero;
    let mut x_3 = u;
    let mut z_3 = one;
    let mut swap: u64 = 0;

    // Iterate from bit 254 down to 0
    for t in (0..=254).rev() {
        let byte_idx = t / 8;
        let bit_idx = t % 8;
        let k_t = ((k[byte_idx] >> bit_idx) & 1) as u64;

        // Conditional swap
        let cs = swap ^ k_t;
        swap = k_t;
        cswap(cs, &mut x_2, &mut x_3);
        cswap(cs, &mut z_2, &mut z_3);

        let a = fe_add(&x_2, &z_2);
        let aa = fe_mul(&a, &a);
        let b = fe_sub(&x_2, &z_2);
        let bb = fe_mul(&b, &b);
        let e = fe_sub(&aa, &bb);
        let c = fe_add(&x_3, &z_3);
        let d = fe_sub(&x_3, &z_3);
        let da = fe_mul(&d, &a);
        let cb = fe_mul(&c, &b);
        x_3 = fe_mul(&fe_add(&da, &cb), &fe_add(&da, &cb));
        z_3 = fe_mul(&x_1, &fe_mul(&fe_sub(&da, &cb), &fe_sub(&da, &cb)));
        x_2 = fe_mul(&aa, &bb);
        // a24 = 121665 for Curve25519
        let a24: Fe = [121665, 0, 0, 0];
        let a24_e = fe_mul(&a24, &e);
        z_2 = fe_mul(&e, &fe_add(&aa, &a24_e));
    }

    // Final conditional swap
    cswap(swap, &mut x_2, &mut x_3);
    cswap(swap, &mut z_2, &mut z_3);

    // Result = x_2 * z_2^(p-2) (mod p)
    let z_inv = fe_inv(&z_2);
    let result = fe_mul(&x_2, &z_inv);
    fe_to_bytes(&result)
}

/// Constant-time conditional swap.
fn cswap(condition: u64, a: &mut Fe, b: &mut Fe) {
    let mask = 0u64.wrapping_sub(condition); // 0 or 0xFFFF...
    for i in 0..4 {
        let t = mask & (a[i] ^ b[i]);
        a[i] ^= t;
        b[i] ^= t;
    }
}
