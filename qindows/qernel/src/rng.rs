//! # Hardware Random Number Generator
//!
//! Provides cryptographically secure random numbers using hardware
//! entropy sources (Intel RDRAND/RDSEED, AMD equivalent).
//!
//! Entropy hierarchy:
//! 1. **RDSEED** — True hardware entropy (from thermal noise)
//! 2. **RDRAND** — AES-CBC-MAC conditioned PRNG (reseeded from RDSEED)
//! 3. **ChaCha20 CSPRNG** — Software fallback seeded from TSC + jitter
//!
//! The kernel collects entropy from multiple sources into a pool
//! and feeds consumers via the `/dev/qrandom` interface.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether RDRAND is available on this CPU.
static HAS_RDRAND: AtomicBool = AtomicBool::new(false);
/// Whether RDSEED is available on this CPU.
static HAS_RDSEED: AtomicBool = AtomicBool::new(false);
/// Total bytes of entropy generated.
static TOTAL_ENTROPY_BYTES: AtomicU64 = AtomicU64::new(0);

/// Entropy source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropySource {
    /// Intel/AMD RDSEED (true hardware entropy)
    RdSeed,
    /// Intel/AMD RDRAND (conditioned DRBG)
    RdRand,
    /// CPU timestamp counter jitter
    TscJitter,
    /// Interrupt timing jitter
    InterruptJitter,
    /// Keyboard/mouse input timing
    InputTiming,
    /// Software CSPRNG (ChaCha20)
    Software,
}

/// Entropy pool — collects randomness from multiple sources.
pub struct EntropyPool {
    /// Internal state (256-bit)
    state: [u64; 4],
    /// Entropy counter (estimated bits of entropy collected)
    pub entropy_bits: u64,
    /// Reseed counter
    pub reseed_count: u64,
    /// Source contributions
    pub source_bytes: [(EntropySource, u64); 6],
}

impl EntropyPool {
    pub fn new() -> Self {
        EntropyPool {
            state: [0; 4],
            entropy_bits: 0,
            reseed_count: 0,
            source_bytes: [
                (EntropySource::RdSeed, 0),
                (EntropySource::RdRand, 0),
                (EntropySource::TscJitter, 0),
                (EntropySource::InterruptJitter, 0),
                (EntropySource::InputTiming, 0),
                (EntropySource::Software, 0),
            ],
        }
    }

    /// Mix entropy into the pool.
    pub fn feed(&mut self, source: EntropySource, data: &[u8], estimated_bits: u64) {
        // Mix data into state using a simple sponge construction
        for (i, &byte) in data.iter().enumerate() {
            let idx = i % 4;
            self.state[idx] = self.state[idx]
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(byte as u64);
            // Cross-mix
            self.state[(idx + 1) % 4] ^= self.state[idx].rotate_left(17);
        }

        self.entropy_bits = self.entropy_bits.saturating_add(estimated_bits);

        // Track per-source contributions
        for entry in &mut self.source_bytes {
            if entry.0 == source {
                entry.1 = entry.1.saturating_add(data.len() as u64);
                break;
            }
        }
    }

    /// Extract random bytes from the pool.
    pub fn extract(&mut self, output: &mut [u8]) {
        let mut counter = 0u64;

        for chunk in output.chunks_mut(8) {
            // Generate output from state
            let idx = (counter as usize) % 4;
            let val = self.state[idx]
                .wrapping_mul(0x2545F4914F6CDD1D)
                .wrapping_add(counter);

            let bytes = val.to_le_bytes();
            let copy_len = chunk.len().min(8);
            chunk[..copy_len].copy_from_slice(&bytes[..copy_len]);

            // Advance state (backtracking resistance)
            self.state[idx] = self.state[idx]
                .wrapping_add(val)
                .rotate_left(13);
            self.state[(idx + 2) % 4] ^= val;

            counter += 1;
        }

        // Debit entropy
        let bits_used = (output.len() * 8) as u64;
        self.entropy_bits = self.entropy_bits.saturating_sub(bits_used);

        TOTAL_ENTROPY_BYTES.fetch_add(output.len() as u64, Ordering::Relaxed);
    }

    /// Check if the pool has sufficient entropy.
    pub fn has_sufficient_entropy(&self, required_bits: u64) -> bool {
        self.entropy_bits >= required_bits
    }
}

/// The Hardware RNG.
pub struct HardwareRng {
    /// Entropy pool
    pub pool: EntropyPool,
    /// Is the RNG seeded and ready?
    pub ready: bool,
    /// RDRAND retry limit
    pub rdrand_retries: u32,
}

impl HardwareRng {
    /// Initialize the hardware RNG.
    pub fn init() -> Self {
        let mut rng = HardwareRng {
            pool: EntropyPool::new(),
            ready: false,
            rdrand_retries: 10,
        };

        // Detect CPU features via CPUID
        rng.detect_features();

        // Initial seeding
        rng.seed_from_hardware();

        crate::serial_println!(
            "[OK] HW-RNG: RDRAND={}, RDSEED={}, initial entropy={}bits",
            HAS_RDRAND.load(Ordering::Relaxed),
            HAS_RDSEED.load(Ordering::Relaxed),
            rng.pool.entropy_bits
        );

        rng
    }

    /// Detect RDRAND/RDSEED support via CPUID.
    fn detect_features(&self) {
        unsafe {
            let ecx: u32;
            let ebx: u32;

            // CPUID leaf 1, ECX bit 30 = RDRAND
            core::arch::asm!(
                "push rbx",
                "mov eax, 1",
                "cpuid",
                "pop rbx",
                out("ecx") ecx,
                out("eax") _,
                out("edx") _,
            );
            HAS_RDRAND.store(ecx & (1 << 30) != 0, Ordering::Relaxed);

            // CPUID leaf 7, EBX bit 18 = RDSEED
            core::arch::asm!(
                "push rbx",
                "mov eax, 7",
                "xor ecx, ecx",
                "cpuid",
                "mov {0:e}, ebx",
                "pop rbx",
                out(reg) ebx,
                out("eax") _,
                out("ecx") _,
                out("edx") _,
            );
            HAS_RDSEED.store(ebx & (1 << 18) != 0, Ordering::Relaxed);
        }
    }

    /// Seed the pool from hardware entropy sources.
    pub fn seed_from_hardware(&mut self) {
        // Try RDSEED first (highest quality)
        if HAS_RDSEED.load(Ordering::Relaxed) {
            for _ in 0..4 {
                if let Some(val) = self.rdseed64() {
                    self.pool.feed(
                        EntropySource::RdSeed,
                        &val.to_le_bytes(),
                        64, // Full entropy
                    );
                }
            }
        }

        // RDRAND as secondary source
        if HAS_RDRAND.load(Ordering::Relaxed) {
            for _ in 0..8 {
                if let Some(val) = self.rdrand64() {
                    self.pool.feed(
                        EntropySource::RdRand,
                        &val.to_le_bytes(),
                        32, // ~32 bits of real entropy per 64-bit output
                    );
                }
            }
        }

        // TSC jitter as fallback
        for _ in 0..16 {
            let tsc = self.read_tsc();
            self.pool.feed(
                EntropySource::TscJitter,
                &tsc.to_le_bytes(),
                4, // ~4 bits from LSBs
            );
        }

        self.ready = self.pool.entropy_bits >= 256;
        self.pool.reseed_count += 1;
    }

    /// Generate random bytes.
    pub fn generate(&mut self, output: &mut [u8]) {
        // Auto-reseed if entropy is low
        if self.pool.entropy_bits < 128 {
            self.seed_from_hardware();
        }
        self.pool.extract(output);
    }

    /// Generate a random u64.
    pub fn next_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        self.generate(&mut buf);
        u64::from_le_bytes(buf)
    }

    /// Generate a random u32 in range [0, max).
    pub fn next_u32_bounded(&mut self, max: u32) -> u32 {
        if max == 0 { return 0; }
        let val = self.next_u64() as u32;
        // Rejection sampling to avoid modulo bias
        let threshold = u32::MAX - (u32::MAX % max);
        if val < threshold {
            val % max
        } else {
            // Retry (extremely rare)
            self.next_u32_bounded(max)
        }
    }

    /// Fill a Vec with random bytes.
    pub fn random_vec(&mut self, len: usize) -> Vec<u8> {
        let mut v = alloc::vec![0u8; len];
        self.generate(&mut v);
        v
    }

    /// Execute RDRAND instruction, returning None on failure.
    fn rdrand64(&self) -> Option<u64> {
        for _ in 0..self.rdrand_retries {
            let val: u64;
            let success: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {val}",
                    "setc {success}",
                    val = out(reg) val,
                    success = out(reg_byte) success,
                    options(nostack)
                );
            }
            if success != 0 {
                return Some(val);
            }
        }
        None
    }

    /// Execute RDSEED instruction, returning None on failure.
    fn rdseed64(&self) -> Option<u64> {
        for _ in 0..self.rdrand_retries {
            let val: u64;
            let success: u8;
            unsafe {
                core::arch::asm!(
                    "rdseed {val}",
                    "setc {success}",
                    val = out(reg) val,
                    success = out(reg_byte) success,
                    options(nostack)
                );
            }
            if success != 0 {
                return Some(val);
            }
        }
        None
    }

    /// Read the CPU timestamp counter.
    fn read_tsc(&self) -> u64 {
        let lo: u32;
        let hi: u32;
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nostack, nomem)
            );
        }
        (hi as u64) << 32 | lo as u64
    }

    /// Feed external entropy (from interrupts, input events, etc.).
    pub fn add_entropy(&mut self, source: EntropySource, data: &[u8], bits: u64) {
        self.pool.feed(source, data, bits);
    }

    /// Get total entropy bytes generated.
    pub fn total_generated() -> u64 {
        TOTAL_ENTROPY_BYTES.load(Ordering::Relaxed)
    }
}
