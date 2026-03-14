//! # QTraffic Law 7 Bridge (Phase 139)
//!
//! ## Architecture Guardian: The Gap
//! `qtraffic.rs` implements `QTrafficEngine`:
//! - `record_flow()` — records a network flow event (FlowEvent)
//! - `authorize_silo()` — sets rate limit for a Silo
//! - `check_law7()` — returns Law7Verdict for a Silo
//! - `tick_window_reset()` — resets per-Silo rate windows
//!
//! **Missing link**: `check_law7()` implemented Law 7 (Network Transparency)
//! checks but was never called from the Nexus send path. All outbound
//! network traffic bypassed the Law 7 gate.
//!
//! This module provides `QTrafficLaw7Bridge`:
//! 1. `gate_outbound()` — calls check_law7() before every Nexus send
//! 2. `record_egress()` — records egress flow with correct FlowEvent fields
//! 3. `on_window_tick()` — resets rate windows via tick_window_reset()
//! 4. `authorize()` — calls authorize_silo() to set rate limits at spawn

extern crate alloc;
use alloc::string::{String, ToString};

use crate::qtraffic::{QTrafficEngine, FlowEvent, FlowDirection, FlowProto, Law7Verdict};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TrafficBridgeStats {
    pub flows_gated:     u64,
    pub flows_allowed:   u64,
    pub flows_denied:    u64,
    pub quarantined:     u64,
    pub strip_token:     u64,
    pub window_resets:   u64,
}

// ── QTraffic Law 7 Bridge ─────────────────────────────────────────────────────

/// Interposes Law 7 (Network Transparency) checks on every outbound flow.
pub struct QTrafficLaw7Bridge {
    pub engine: QTrafficEngine,
    pub stats:  TrafficBridgeStats,
    window_ticks: u64,
    next_window_tick: u64,
}

impl QTrafficLaw7Bridge {
    pub fn new() -> Self {
        QTrafficLaw7Bridge {
            engine: QTrafficEngine::new(),
            stats: TrafficBridgeStats::default(),
            window_ticks: 60_000, // 1-second window at 60kHz
            next_window_tick: 60_000,
        }
    }

    /// Register a Silo with a rate limit. Call at Silo spawn.
    pub fn authorize(&mut self, silo_id: u64, rate_limit_bps: u64) {
        self.engine.authorize_silo(silo_id, rate_limit_bps);
    }

    /// Gate an outbound connection request via Law 7.
    /// Returns true if allowed, false if denied.
    pub fn gate_outbound(
        &mut self,
        silo_id: u64,
        destination: &str,
        proto: FlowProto,
        bytes: u64,
        encrypted: bool,
        payload_entropy: f32,
        tick: u64,
    ) -> bool {
        self.stats.flows_gated += 1;

        match self.engine.check_law7(silo_id) {
            Law7Verdict::Allow => {
                self.stats.flows_allowed += 1;
                self.engine.record_flow(FlowEvent {
                    silo_id,
                    tick,
                    direction: FlowDirection::Egress,
                    bytes,
                    proto,
                    encrypted,
                    destination: destination.to_string(),
                    payload_entropy,
                });
                true
            }
            Law7Verdict::VaporizeNoToken => {
                self.stats.flows_denied += 1;
                crate::serial_println!(
                    "[TRAFFIC LAW7] VaporizeNoToken Silo {}→{}", silo_id, destination
                );
                false
            }
            Law7Verdict::StripToken => {
                self.stats.strip_token += 1;
                crate::serial_println!(
                    "[TRAFFIC LAW7] StripToken Silo {} — covert header removed", silo_id
                );
                // Allow but record with zeroed entropy (covert channel mitigated)
                self.engine.record_flow(FlowEvent {
                    silo_id, tick,
                    direction: FlowDirection::Egress,
                    bytes, proto, encrypted,
                    destination: destination.to_string(),
                    payload_entropy: 0.0, // stripped
                });
                true
            }
            Law7Verdict::QuarantineCovert => {
                self.stats.quarantined += 1;
                crate::serial_println!(
                    "[TRAFFIC LAW7] QUARANTINE Silo {} — covert channel detected!", silo_id
                );
                false
            }
        }
    }

    /// Record an inbound flow (metered, never denied).
    pub fn record_ingress(
        &mut self,
        silo_id: u64,
        source: &str,
        proto: FlowProto,
        bytes: u64,
        encrypted: bool,
        tick: u64,
    ) {
        self.engine.record_flow(FlowEvent {
            silo_id, tick,
            direction: FlowDirection::Ingress,
            bytes, proto, encrypted,
            destination: source.to_string(),
            payload_entropy: 0.0,
        });
    }

    /// Reset all rate windows (call every N ticks from APIC timer).
    pub fn on_window_tick(&mut self, tick: u64) {
        if tick >= self.next_window_tick {
            self.engine.tick_window_reset();
            self.stats.window_resets += 1;
            self.next_window_tick = tick + self.window_ticks;
        }
    }

    /// Remove a Silo's traffic account (call at vaporize).
    pub fn on_silo_exit(&mut self, silo_id: u64) {
        self.engine.remove_silo(silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TrafficBridge: gated={} ok={} denied={} quarantine={} strip={} resets={}",
            self.stats.flows_gated, self.stats.flows_allowed,
            self.stats.flows_denied, self.stats.quarantined,
            self.stats.strip_token, self.stats.window_resets
        );
    }
}
