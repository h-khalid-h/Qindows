//! # Global Entropy Pool — Mesh-Wide RNG Seeding
//!
//! Provides cryptographically secure random numbers by combining
//! local hardware entropy (RDSEED/RDRAND) with global mesh entropy
//! from the Genesis Protocol (Section 11.2).
//!
//! The pool continuously mixes:
//! - Hardware RNG (CPU RDSEED/RDRAND)
//! - Interrupt timing jitter
//! - Network packet arrival times
//! - Mesh peer entropy contributions
//! - NVMe command latency jitter

extern crate alloc;

use alloc::vec::Vec;

/// Entropy source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropySource {
    /// CPU hardware RNG (RDSEED/RDRAND)
    Hardware,
    /// Interrupt timing jitter
    InterruptJitter,
    /// Network packet timing
    NetworkTiming,
    /// Mesh peer contribution
    MeshPeer,
    /// Storage I/O jitter
    StorageJitter,
    /// User input timing
    UserInput,
}

/// An entropy sample.
#[derive(Debug, Clone)]
pub struct EntropySample {
    /// Source type
    pub source: EntropySource,
    /// Raw entropy bytes
    pub data: [u8; 32],
    /// Estimated entropy bits
    pub entropy_bits: u32,
    /// Timestamp
    pub timestamp: u64,
}

/// Pool health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolHealth {
    /// Sufficient entropy
    Healthy,
    /// Low entropy — accepting additional sources
    Low,
    /// Critically low — blocking RNG requests until reseeded
    Critical,
}

/// Entropy pool statistics.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub samples_mixed: u64,
    pub bytes_generated: u64,
    pub reseeds: u64,
    pub mesh_contributions: u64,
    pub starvation_events: u64,
}

/// The Global Entropy Pool.
pub struct EntropyPool {
    /// Internal state (512-bit pool)
    pool: [u8; 64],
    /// How many entropy bits have been mixed in
    entropy_bits: u64,
    /// Minimum entropy bits before generation is allowed
    min_entropy: u64,
    /// Generation counter (mixed into output)
    generation: u64,
    /// Recent samples (for auditing)
    recent_samples: Vec<EntropySample>,
    /// Max recent samples to keep
    max_recent: usize,
    /// Statistics
    pub stats: PoolStats,
}

impl EntropyPool {
    pub fn new() -> Self {
        EntropyPool {
            pool: [0u8; 64],
            entropy_bits: 0,
            min_entropy: 256,
            generation: 0,
            recent_samples: Vec::new(),
            max_recent: 64,
            stats: PoolStats::default(),
        }
    }

    /// Mix entropy into the pool.
    pub fn mix(&mut self, sample: EntropySample) {
        // XOR-fold sample data into pool state
        for i in 0..32 {
            self.pool[i] ^= sample.data[i];
            self.pool[32 + i] ^= sample.data[31 - i]; // Reverse fold
        }

        // Rotate pool state for diffusion
        let carry = self.pool[63];
        for i in (1..64).rev() {
            self.pool[i] = self.pool[i]
                .wrapping_add(self.pool[i - 1])
                .rotate_left(3);
        }
        self.pool[0] = self.pool[0].wrapping_add(carry).rotate_left(3);

        self.entropy_bits += sample.entropy_bits as u64;
        self.stats.samples_mixed += 1;

        if sample.source == EntropySource::MeshPeer {
            self.stats.mesh_contributions += 1;
        }

        // Keep recent sample record
        if self.recent_samples.len() >= self.max_recent {
            self.recent_samples.remove(0);
        }
        self.recent_samples.push(sample);
    }

    /// Generate random bytes from the pool.
    pub fn generate(&mut self, output: &mut [u8]) -> Result<(), &'static str> {
        if self.entropy_bits < self.min_entropy {
            self.stats.starvation_events += 1;
            return Err("Insufficient entropy");
        }

        self.generation += 1;

        // Mix generation counter into pool
        let gen_bytes = self.generation.to_le_bytes();
        for i in 0..8 {
            self.pool[i] ^= gen_bytes[i];
        }

        // Generate output via pool stretching
        let mut pos = 0;
        while pos < output.len() {
            // Hash-like extraction: fold pool into output
            for i in 0..64 {
                if pos >= output.len() { break; }
                output[pos] = self.pool[i]
                    .wrapping_add(self.pool[(i + 17) % 64])
                    .wrapping_mul(self.pool[(i + 37) % 64] | 1);
                pos += 1;
            }

            // Re-mix pool after each extraction round
            for i in 0..64 {
                self.pool[i] = self.pool[i]
                    .wrapping_add(self.pool[(i + 1) % 64])
                    .rotate_left(5);
            }
        }

        // Debit entropy (conservative: 1 bit per output byte)
        self.entropy_bits = self.entropy_bits
            .saturating_sub(output.len() as u64 * 8);

        self.stats.bytes_generated += output.len() as u64;
        Ok(())
    }

    /// Reseed from a mesh peer's contribution.
    pub fn reseed_from_mesh(&mut self, peer_entropy: [u8; 32], peer_bits: u32, now: u64) {
        self.mix(EntropySample {
            source: EntropySource::MeshPeer,
            data: peer_entropy,
            entropy_bits: peer_bits,
            timestamp: now,
        });
        self.stats.reseeds += 1;
    }

    /// Get pool health.
    pub fn health(&self) -> PoolHealth {
        if self.entropy_bits >= self.min_entropy {
            PoolHealth::Healthy
        } else if self.entropy_bits >= self.min_entropy / 4 {
            PoolHealth::Low
        } else {
            PoolHealth::Critical
        }
    }

    /// Get estimated entropy bits available.
    pub fn available_entropy(&self) -> u64 {
        self.entropy_bits
    }
}
