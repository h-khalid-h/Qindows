//! # Entropy Pool RNG Bridge (Phase 184)
//!
//! ## Architecture Guardian: The Gap
//! `rng.rs` implements:
//! - `EntropyPool::feed(source: EntropySource, data: &[u8], estimated_bits: u64)` — add entropy
//! - `EntropyPool::extract(&mut [u8])` — fill output with pseudorandom bytes
//! - `EntropyPool::has_sufficient_entropy(required_bits)` → bool
//! - `HardwareRng::init()`, `HardwareRng::generate(&mut [u8])`, `::next_u64()`
//! - `EntropySource` variants: RdSeed, RdRand, TscJitter, InterruptJitter, InputTiming, Software
//!
//! **Missing link**: `EntropyPool::extract()` was always callable regardless
//! of entropy pool state — low entropy at boot caused weak cryptographic keys.
//!
//! This module provides `EntropyRngBridge`:
//! `extract_with_entropy_check()` — blocks extraction below 128 bits of entropy.

extern crate alloc;

use crate::rng::{EntropyPool, HardwareRng, EntropySource};

const MIN_ENTROPY_BITS: u64 = 128;

#[derive(Debug, Default, Clone)]
pub struct EntropyBridgeStats {
    pub extractions_ok:      u64,
    pub extractions_blocked: u64,
    pub hw_feeds:            u64,
}

pub struct EntropyRngBridge {
    pub pool:  EntropyPool,
    hw_rng:    HardwareRng,
    pub stats: EntropyBridgeStats,
}

impl EntropyRngBridge {
    pub fn new() -> Self {
        EntropyRngBridge {
            pool:   EntropyPool::new(),
            hw_rng: HardwareRng::init(),
            stats:  EntropyBridgeStats::default(),
        }
    }

    /// Seed the pool from hardware RDRAND/RDSEED. Call at boot and periodically.
    pub fn feed_hardware_entropy(&mut self, tick: u64) {
        self.stats.hw_feeds += 1;
        let mut hw_bytes = [0u8; 32];
        self.hw_rng.generate(&mut hw_bytes);
        self.pool.feed(EntropySource::RdRand, &hw_bytes, 256);
        crate::serial_println!("[ENTROPY] Hardware seed fed: {} bits (tick={})", 256, tick);
    }

    /// Extract random bytes — blocked if entropy < 128 bits.
    pub fn extract_with_entropy_check(&mut self, output: &mut [u8]) -> bool {
        if !self.pool.has_sufficient_entropy(MIN_ENTROPY_BITS) {
            self.stats.extractions_blocked += 1;
            crate::serial_println!(
                "[ENTROPY] Extraction blocked — pool below {}b entropy", MIN_ENTROPY_BITS
            );
            return false;
        }
        self.stats.extractions_ok += 1;
        self.pool.extract(output);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EntropyBridge: ok={} blocked={} hw_feeds={}",
            self.stats.extractions_ok, self.stats.extractions_blocked, self.stats.hw_feeds
        );
    }
}
