//! # Network Rate Limiter Silo Bridge (Phase 170)
//!
//! ## Architecture Guardian: The Gap
//! `q_traffic.rs` provides `QTrafficEngine` / Law 7 covert-channel detection.
//! The existing `QTrafficLaw7Bridge` detects violations — but never enforced
//! a hard bytes/sec rate limit per Silo at the network layer.
//!
//! **Missing link**: After QTrafficEngine flagged a Silo for high-bandwidth
//! anomaly, nothing actually throttled subsequent packets. The Sentinel could
//! quarantine eventually, but in the window between detection and quarantine
//! a Silo could exfiltrate unbounded amounts of data.
//!
//! This module provides `NetworkRateSiloBridge`:
//! 1. `on_silo_spawn()` — set per-Silo rate limits
//! 2. `check_packet()` — enforce rate limit before packet is forwarded
//! 3. `on_traffic_anomaly()` — halve the rate limit on detected anomaly

extern crate alloc;
use alloc::collections::BTreeMap;

#[derive(Debug, Clone)]
struct SiloRateState {
    limit_bytes_per_tick: u64,
    used_this_tick:       u64,
    anomaly_strikes:      u32,
}

#[derive(Debug, Default, Clone)]
pub struct NetworkRateBridgeStats {
    pub packets_allowed: u64,
    pub packets_dropped: u64,
    pub silos_throttled: u64,
}

pub struct NetworkRateSiloBridge {
    rates:  BTreeMap<u64, SiloRateState>,
    pub stats: NetworkRateBridgeStats,
    /// Default per-tick byte limit (approx 10MiB/s at 1000 ticks/s)
    default_limit: u64,
}

impl NetworkRateSiloBridge {
    pub fn new() -> Self {
        NetworkRateSiloBridge {
            rates: BTreeMap::new(),
            stats: NetworkRateBridgeStats::default(),
            default_limit: 10 * 1024 * 1024 / 1000, // 10KiB/tick
        }
    }

    /// Set per-Silo rate limit at spawn.
    pub fn on_silo_spawn(&mut self, silo_id: u64, limit_bytes_per_tick: u64) {
        self.rates.insert(silo_id, SiloRateState {
            limit_bytes_per_tick,
            used_this_tick: 0,
            anomaly_strikes: 0,
        });
    }

    /// Check and charge a packet. Returns false if rate limit exceeded.
    pub fn check_packet(&mut self, silo_id: u64, packet_bytes: u64) -> bool {
        let limit = self.default_limit;
        let state = self.rates.entry(silo_id).or_insert(SiloRateState {
            limit_bytes_per_tick: limit,
            used_this_tick: 0,
            anomaly_strikes: 0,
        });

        if state.used_this_tick + packet_bytes > state.limit_bytes_per_tick {
            self.stats.packets_dropped += 1;
            return false;
        }
        state.used_this_tick += packet_bytes;
        self.stats.packets_allowed += 1;
        true
    }

    /// Reset per-tick counters (call once per scheduler tick).
    pub fn on_tick_reset(&mut self) {
        for state in self.rates.values_mut() {
            state.used_this_tick = 0;
        }
    }

    /// Halve the rate limit on QTrafficEngine anomaly detection.
    pub fn on_traffic_anomaly(&mut self, silo_id: u64) {
        if let Some(state) = self.rates.get_mut(&silo_id) {
            state.limit_bytes_per_tick /= 2;
            state.anomaly_strikes += 1;
            self.stats.silos_throttled += 1;
            crate::serial_println!(
                "[NET RATE] Silo {} throttled — limit now {}/tick (strike #{})",
                silo_id, state.limit_bytes_per_tick, state.anomaly_strikes
            );
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NetworkRateBridge: allowed={} dropped={} throttled={}",
            self.stats.packets_allowed, self.stats.packets_dropped, self.stats.silos_throttled
        );
    }
}
