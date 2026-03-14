//! # Digital Antibody Rate Bridge (Phase 259)
//!
//! ## Architecture Guardian: The Gap
//! `digital_antibody.rs` implements the Digital Antibody system:
//! - `AntibodyGenerator` — generates antibody definitions from anomalies
//! - `LocalImmunityRegistry` — stores applied antibodies
//!
//! **Missing link**: Antibody generation rate was unthrottled. During a
//! multi-vector attack, the system could generate thousands of antibodies
//! per tick, exhausting the registry and triggering false-positive blocks.
//!
//! This module provides `DigitalAntibodyRateBridge`:
//! Max 8 new antibodies generated per tick.

extern crate alloc;

const MAX_ANTIBODIES_PER_TICK: u64 = 8;

#[derive(Debug, Default, Clone)]
pub struct AntibodyRateStats {
    pub generated_ok: u64,
    pub throttled:    u64,
}

pub struct DigitalAntibodyRateBridge {
    generated_this_tick: u64,
    current_tick:        u64,
    pub stats:           AntibodyRateStats,
}

impl DigitalAntibodyRateBridge {
    pub fn new() -> Self {
        DigitalAntibodyRateBridge { generated_this_tick: 0, current_tick: 0, stats: AntibodyRateStats::default() }
    }

    pub fn allow_generate(&mut self, tick: u64) -> bool {
        if tick != self.current_tick {
            self.generated_this_tick = 0;
            self.current_tick = tick;
        }
        if self.generated_this_tick >= MAX_ANTIBODIES_PER_TICK {
            self.stats.throttled += 1;
            crate::serial_println!("[ANTIBODY] rate limit reached ({}/{})", self.generated_this_tick, MAX_ANTIBODIES_PER_TICK);
            return false;
        }
        self.generated_this_tick += 1;
        self.stats.generated_ok += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AntibodyRateBridge: ok={} throttled={}", self.stats.generated_ok, self.stats.throttled
        );
    }
}
