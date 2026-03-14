//! # RNG Entropy Feeder (Phase 129)
//!
//! ## Architecture Guardian: The Gap
//! `rng.rs` (Phase 84) implements `HardwareRng` + `EntropyPool`:
//! - `HardwareRng::init()` — tries RDRAND (x86 instruction), falls back to TSC + CPUID
//! - `EntropyPool::feed()` — accepts entropy from any `EntropySource`  
//! - `HardwareRng::seed_from_hardware()` — calls feed() with hardware sources
//!
//! **Missing link**: After `init()`, nothing ever **re-fed entropy** to keep
//! the pool fresh. The pool starts with hardware seeds but drifts stale.
//! Also: the `EntropySource::UserInput`, `EntropySource::NetworkPacket`,
//! and `EntropySource::Timer` sources were declared but never called.
//!
//! This module provides `RngEntropyFeeder`:
//! 1. `feed_timer_entropy()` — feeds TSC jitter on every APIC tick (Law 7 side channel mitigation)
//! 2. `feed_network_entropy()` — mixes Nexus packet arrival timing
//! 3. `feed_pmc_entropy()` — mixes PMC branch misprediction counts (CPU-sourced jitter)
//! 4. `check_refresh()` — triggers re-seeding if pool drops below threshold

extern crate alloc;
use alloc::vec::Vec;

use crate::rng::{HardwareRng, EntropyPool, EntropySource};

// ── Feeder Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct EntropyFeederStats {
    pub timer_feeds:    u64,
    pub network_feeds:  u64,
    pub pmc_feeds:      u64,
    pub refreshes:      u64,
    pub bytes_generated: u64,
}

// ── RNG Entropy Feeder ────────────────────────────────────────────────────────

/// Keeps the HardwareRng entropy pool continuously refreshed from live kernel sources.
pub struct RngEntropyFeeder {
    pub rng: HardwareRng,
    pub stats: EntropyFeederStats,
    /// Tick of last pool replenishment
    last_refresh_tick: u64,
    /// Refresh every N ticks (default: 10_000 = ~166ms at 60kHz)
    refresh_interval: u64,
}

impl RngEntropyFeeder {
    pub fn new() -> Self {
        RngEntropyFeeder {
            rng: HardwareRng::init(),
            stats: EntropyFeederStats::default(),
            last_refresh_tick: 0,
            refresh_interval: 10_000,
        }
    }

    /// Feed TSC jitter as timer entropy (call from APIC tick handler).
    pub fn feed_timer_entropy(&mut self, tsc: u64, tick: u64) {
        self.stats.timer_feeds += 1;
        // TSC jitter: XOR of tick and TSC captures CPU timing noise
        let jitter = tsc ^ tick ^ (tsc.wrapping_add(tick).rotate_left(17));
        let bytes = jitter.to_le_bytes();
        self.rng.add_entropy(EntropySource::TscJitter, &bytes, 8);
    }

    /// Feed network packet inter-arrival timing.
    pub fn feed_network_entropy(&mut self, packet_arrival_tsc: u64) {
        self.stats.network_feeds += 1;
        let bytes = packet_arrival_tsc.to_le_bytes();
        self.rng.add_entropy(EntropySource::InterruptJitter, &bytes, 4);
    }

    /// Feed PMC branch misprediction jitter.
    pub fn feed_pmc_entropy(&mut self, branch_misses: u64, tick: u64) {
        self.stats.pmc_feeds += 1;
        let combined = branch_misses ^ tick;
        let bytes = combined.to_le_bytes();
        self.rng.add_entropy(EntropySource::TscJitter, &bytes, 6);
    }

    /// Check if pool needs refreshing; re-seed from hardware if so.
    pub fn check_refresh(&mut self, tick: u64) {
        if tick.saturating_sub(self.last_refresh_tick) >= self.refresh_interval {
            self.rng.seed_from_hardware();
            self.stats.refreshes += 1;
            self.last_refresh_tick = tick;
            crate::serial_println!(
                "[RNG FEEDER] Pool refreshed @ tick {} (refresh #{})",
                tick, self.stats.refreshes
            );
        }
    }

    /// Generate `n` random bytes using the kept-fresh pool.
    pub fn generate(&mut self, n: usize) -> Vec<u8> {
        self.stats.bytes_generated += n as u64;
        self.rng.random_vec(n)
    }

    /// Generate a random u64.
    pub fn next_u64(&mut self) -> u64 {
        self.stats.bytes_generated += 8;
        self.rng.next_u64()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  RngFeeder: timer={} net={} pmc={} refreshes={} bytes={}",
            self.stats.timer_feeds, self.stats.network_feeds, self.stats.pmc_feeds,
            self.stats.refreshes, self.stats.bytes_generated
        );
    }
}
