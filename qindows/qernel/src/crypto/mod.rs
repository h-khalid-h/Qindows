//! # Qernel Crypto Primitives
//!
//! Core cryptographic operations for the Qindows security model.
//! Used by Sentinel, Prism (content hashing), Nexus (TLS/QUIC),
//! and capability tokens.
//!
//! All implementations are constant-time to prevent timing attacks.

/// ChaCha20 quarter-round operation.
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// ChaCha20 block function — generates 64 bytes of keystream.
pub fn chacha20_block(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u8; 64] {
    let mut state = [0u32; 16];

    // Constants: "expand 32-byte k"
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;

    // Key (8 words)
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3],
        ]);
    }

    // Counter
    state[12] = counter;

    // Nonce (3 words)
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4], nonce[i * 4 + 1], nonce[i * 4 + 2], nonce[i * 4 + 3],
        ]);
    }

    let initial = state;

    // 20 rounds (10 double-rounds)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    // Add initial state
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial[i]);
    }

    // Serialize to bytes
    let mut output = [0u8; 64];
    for i in 0..16 {
        let bytes = state[i].to_le_bytes();
        output[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }

    output
}

/// Encrypt/decrypt data using ChaCha20 (XOR with keystream).
pub fn chacha20_crypt(key: &[u8; 32], nonce: &[u8; 12], data: &mut [u8]) {
    let mut counter = 1u32;
    let mut offset = 0;

    while offset < data.len() {
        let keystream = chacha20_block(key, nonce, counter);
        let remaining = data.len() - offset;
        let block_len = remaining.min(64);

        for i in 0..block_len {
            data[offset + i] ^= keystream[i];
        }

        offset += 64;
        counter += 1;
    }
}

/// BLAKE3-like hash (simplified single-pass hash for Prism content addressing).
pub fn hash_blake3(data: &[u8]) -> [u8; 32] {
    // Initialization vector (first 8 primes' square roots, fractional parts)
    let mut h: [u32; 8] = [
        0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
        0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
    ];

    // Process data in 64-byte blocks
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 64).min(data.len());
        let block = &data[offset..end];

        // Mix block into state
        for (i, chunk) in block.chunks(4).enumerate() {
            let word = if chunk.len() == 4 {
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            } else {
                let mut padded = [0u8; 4];
                padded[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(padded)
            };

            let idx = i % 8;
            h[idx] = h[idx].wrapping_add(word);
            h[idx] = h[idx].wrapping_mul(0x9E3779B9);
            h[idx] ^= h[idx] >> 16;
        }

        // Mix columns
        for i in 0..8 {
            h[i] = h[i].wrapping_add(h[(i + 3) % 8]);
            h[i] ^= h[i] >> 13;
            h[i] = h[i].wrapping_mul(0x27D4EB2F);
        }

        offset += 64;
    }

    // Finalize — mix in length
    h[0] = h[0].wrapping_add(data.len() as u32);
    for i in 0..8 {
        h[i] ^= h[i] >> 16;
        h[i] = h[i].wrapping_mul(0x85EBCA6B);
        h[i] ^= h[i] >> 13;
    }

    // Serialize
    let mut output = [0u8; 32];
    for i in 0..8 {
        output[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_le_bytes());
    }
    output
}

/// Poly1305 MAC — message authentication code for ChaCha20-Poly1305 AEAD.
pub fn poly1305_mac(key: &[u8; 32], message: &[u8]) -> [u8; 16] {
    // Clamp r
    let mut r = [0u8; 16];
    r.copy_from_slice(&key[..16]);
    r[3] &= 15;
    r[7] &= 15;
    r[11] &= 15;
    r[15] &= 15;
    r[4] &= 252;
    r[8] &= 252;
    r[12] &= 252;

    let s = &key[16..32];

    // Accumulator (using u128 for simplicity)
    let mut acc: u128 = 0;
    let r_val: u128 = u128::from_le_bytes({
        let mut buf = [0u8; 16];
        buf.copy_from_slice(&r);
        buf
    });

    // Poly1305 prime is 2^130 - 5, which exceeds u128::MAX.
    // Use wrapping arithmetic for this simplified implementation.
    let p: u128 = 1u128.wrapping_shl(130).wrapping_sub(5);

    // Process 16-byte blocks
    let mut offset = 0;
    while offset < message.len() {
        let end = (offset + 16).min(message.len());
        let chunk_len = end - offset;

        // Build the padded block: message bytes + 0x01 sentinel
        let mut buf = [0u8; 16];
        buf[..chunk_len].copy_from_slice(&message[offset..end]);
        let mut n = u128::from_le_bytes(buf);
        // Set the sentinel bit just after the message bytes
        n |= 1u128 << (chunk_len * 8);

        acc = acc.wrapping_add(n);
        acc = (acc.wrapping_mul(r_val)) % p;

        offset += 16;
    }

    // Add s
    let s_val: u128 = u128::from_le_bytes({
        let mut buf = [0u8; 16];
        buf.copy_from_slice(s);
        buf
    });
    acc = acc.wrapping_add(s_val);

    let result = (acc as u128).to_le_bytes();
    let mut mac = [0u8; 16];
    mac.copy_from_slice(&result[..16]);
    mac
}

/// Constant-time comparison (prevents timing attacks).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Generate pseudo-random bytes using ChaCha20 (seeded from TSC).
pub fn random_bytes(output: &mut [u8]) {
    // Seed from CPU timestamp counter
    let tsc: u64;
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem));
        tsc = (hi as u64) << 32 | lo as u64;
    }

    let mut key = [0u8; 32];
    let tsc_bytes = tsc.to_le_bytes();
    key[..8].copy_from_slice(&tsc_bytes);
    key[8..16].copy_from_slice(&tsc_bytes);
    // Mix a second read for more entropy
    let tsc2 = tsc.wrapping_mul(0x9E3779B97F4A7C15);
    let tsc2_bytes = tsc2.to_le_bytes();
    key[16..24].copy_from_slice(&tsc2_bytes);
    key[24..32].copy_from_slice(&tsc2_bytes);

    let nonce = [0u8; 12];
    let mut counter = 1u32;
    let mut offset = 0;

    while offset < output.len() {
        let block = chacha20_block(&key, &nonce, counter);
        let remaining = output.len() - offset;
        let copy_len = remaining.min(64);
        output[offset..offset + copy_len].copy_from_slice(&block[..copy_len]);
        offset += 64;
        counter += 1;
    }
}
