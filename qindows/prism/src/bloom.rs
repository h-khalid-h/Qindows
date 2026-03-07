//! # Prism Bloom Filter
//!
//! Space-efficient probabilistic data structure for membership
//! testing. Used by Prism to avoid unnecessary disk reads during
//! key lookups (negative lookups are guaranteed correct).

extern crate alloc;

use alloc::vec::Vec;
use crate::math_ext::{F32Ext, F64Ext};

/// The Bloom Filter.
pub struct BloomFilter {
    /// Bit array
    bits: Vec<u64>,
    /// Number of bits (m)
    num_bits: usize,
    /// Number of hash functions (k)
    num_hashes: u8,
    /// Items inserted
    pub count: u64,
    /// False positive rate target
    pub target_fpr: f64,
}

impl BloomFilter {
    /// Create a new bloom filter sized for `expected_items` with
    /// a target false positive rate.
    pub fn new(expected_items: usize, fpr: f64) -> Self {
        // m = -n * ln(fpr) / (ln(2)^2)
        let ln2 = 0.693_147_180_559_945_3_f64;
        let m = if expected_items == 0 { 64 } else {
            let m_f = -(expected_items as f64) * fpr.ln() / (ln2 * ln2);
            (m_f as usize).max(64)
        };

        // k = (m/n) * ln(2)
        let k = if expected_items == 0 { 3 } else {
            let k_f = (m as f64 / expected_items as f64) * ln2;
            (k_f as u8).max(1).min(16)
        };

        let word_count = (m + 63) / 64;

        BloomFilter {
            bits: alloc::vec![0u64; word_count],
            num_bits: m,
            num_hashes: k,
            count: 0,
            target_fpr: fpr,
        }
    }

    /// Insert a key.
    pub fn insert(&mut self, key: &[u8]) {
        let (h1, h2) = self.double_hash(key);
        for i in 0..self.num_hashes as u64 {
            let bit = self.nth_hash(h1, h2, i);
            self.set_bit(bit);
        }
        self.count += 1;
    }

    /// Test if a key might be present.
    /// Returns `false` = definitely not present (guaranteed).
    /// Returns `true` = probably present (may be false positive).
    pub fn may_contain(&self, key: &[u8]) -> bool {
        let (h1, h2) = self.double_hash(key);
        for i in 0..self.num_hashes as u64 {
            let bit = self.nth_hash(h1, h2, i);
            if !self.get_bit(bit) {
                return false;
            }
        }
        true
    }

    /// Clear all bits.
    pub fn clear(&mut self) {
        for word in &mut self.bits {
            *word = 0;
        }
        self.count = 0;
    }

    /// Estimated false positive rate given current fill.
    pub fn estimated_fpr(&self) -> f64 {
        let ones = self.popcount() as f64;
        let m = self.num_bits as f64;
        let k = self.num_hashes as f64;
        let fill = ones / m;
        // (1 - e^(-kn/m))^k ≈ fill^k
        let mut result = 1.0;
        for _ in 0..self.num_hashes {
            result *= fill;
        }
        result
    }

    /// Number of set bits.
    pub fn popcount(&self) -> u64 {
        self.bits.iter().map(|w| w.count_ones() as u64).sum()
    }

    /// Fill ratio (0.0 to 1.0).
    pub fn fill_ratio(&self) -> f64 {
        self.popcount() as f64 / self.num_bits as f64
    }

    /// Merge another bloom filter into this one (union).
    pub fn merge(&mut self, other: &BloomFilter) {
        if self.num_bits != other.num_bits || self.num_hashes != other.num_hashes {
            return; // Incompatible
        }
        for (i, word) in other.bits.iter().enumerate() {
            if i < self.bits.len() {
                self.bits[i] |= word;
            }
        }
        self.count += other.count;
    }

    /// Double hashing: produce two independent hashes from a key.
    fn double_hash(&self, key: &[u8]) -> (u64, u64) {
        // FNV-1a for h1
        let mut h1: u64 = 0xcbf29ce484222325;
        for &b in key {
            h1 ^= b as u64;
            h1 = h1.wrapping_mul(0x100000001b3);
        }

        // FNV-1a variant for h2 (different seed)
        let mut h2: u64 = 0x84222325cbf29ce4;
        for &b in key.iter().rev() {
            h2 ^= b as u64;
            h2 = h2.wrapping_mul(0x100000001b3);
        }

        (h1, h2)
    }

    /// Compute the i-th hash from double hashing.
    fn nth_hash(&self, h1: u64, h2: u64, i: u64) -> usize {
        (h1.wrapping_add(i.wrapping_mul(h2)) % self.num_bits as u64) as usize
    }

    fn set_bit(&mut self, pos: usize) {
        let word = pos / 64;
        let bit = pos % 64;
        if word < self.bits.len() {
            self.bits[word] |= 1 << bit;
        }
    }

    fn get_bit(&self, pos: usize) -> bool {
        let word = pos / 64;
        let bit = pos % 64;
        if word < self.bits.len() {
            self.bits[word] & (1 << bit) != 0
        } else {
            false
        }
    }
}
