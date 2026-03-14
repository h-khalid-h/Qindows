//! # Digital Antibody Nexus Bridge (Phase 141)
//!
//! ## Architecture Guardian: The Gap
//! `digital_antibody.rs` implements:
//! - `LocalImmunityRegistry::apply()` — applies antibody payload (local or peer)
//! - `LocalImmunityRegistry::scan()` — matches observed hash against known signatures
//! - `LocalImmunityRegistry::is_binary_blacklisted()` — Law 2 binary integrity check
//! - `AntibodyGenerator::generate()` — creates new antibody from threat evidence
//! - `AntibodyGenerator::drain_broadcast_queue()` — returns payloads to broadcast
//!
//! **Missing links**:
//! 1. `scan()` was never called at Silo spawn time — blacklisted binaries were loaded
//! 2. `drain_broadcast_queue()` was never wired to Nexus — antibodies never propagated
//! 3. `LocalImmunityRegistry` was never connected to `SentinelAnomalyGate`
//!
//! This module provides `DigitalAntibodyBridge`:
//! 1. `on_silo_spawn_check()` — scan binary hash before Silo launch
//! 2. `on_anomaly_detected()` — generate antibody from anomaly score
//! 3. `on_peer_antibody()` — apply received Nexus antibody payload
//! 4. `drain_for_broadcast()` — return payloads for Nexus propagation

extern crate alloc;
use alloc::vec::Vec;

use crate::digital_antibody::{
    LocalImmunityRegistry, AntibodyGenerator, AntibodyPayload,
    AntibodyAction, ThreatCategory,
};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AntibodyBridgeStats {
    pub spawn_checks:      u64,
    pub spawn_blocked:     u64,
    pub antibodies_gen:    u64,
    pub antibodies_recv:   u64,
    pub broadcasts_queued: u64,
}

// ── Digital Antibody Bridge ───────────────────────────────────────────────────

/// Connects LocalImmunityRegistry and AntibodyGenerator to the kernel event bus.
pub struct DigitalAntibodyBridge {
    pub registry:  LocalImmunityRegistry,
    pub generator: AntibodyGenerator,
    pub stats:     AntibodyBridgeStats,
}

impl DigitalAntibodyBridge {
    pub fn new(node_id: u64) -> Self {
        DigitalAntibodyBridge {
            registry:  LocalImmunityRegistry::new(),
            generator: AntibodyGenerator::new(node_id),
            stats:     AntibodyBridgeStats::default(),
        }
    }

    /// Called at Silo spawn: reject if the binary is blacklisted (Law 2).
    pub fn on_silo_spawn_check(&mut self, binary_oid: &[u8; 32], silo_id: u64) -> bool {
        self.stats.spawn_checks += 1;

        if self.registry.is_binary_blacklisted(binary_oid) {
            self.stats.spawn_blocked += 1;
            crate::serial_println!(
                "[ANTIBODY] Silo {} BLOCKED — binary {:02x}{:02x}.. blacklisted (Law 2)",
                silo_id, binary_oid[0], binary_oid[1]
            );
            return false;
        }

        // Also run a full scan for partial signature matches
        if let Some(payload) = self.registry.scan(binary_oid) {
            crate::serial_println!(
                "[ANTIBODY] Silo {} spawn: antibody match id={} cat={:?} sev={}",
                silo_id, payload.antibody_id,
                payload.signature.category, payload.severity
            );
        }

        true
    }

    /// Generate an antibody when PMC-based anomaly is detected.
    pub fn on_anomaly_detected(
        &mut self,
        silo_id: u64,
        observed_hash: [u8; 32],
        anomaly_score: u8,
        tick: u64,
    ) {
        self.stats.antibodies_gen += 1;

        // generate(category, pattern_hash, binary_oid: Option, description, tick) -> u64
        let _antibody_id = self.generator.generate(
            ThreatCategory::Unknown,
            observed_hash,
            None, // no specific binary OID tied to this anomaly
            "Sentinel PMC anomaly",
            tick,
        );

        crate::serial_println!(
            "[ANTIBODY] Generated from Silo {} anomaly score={}", silo_id, anomaly_score
        );
    }

    /// Apply an antibody received from a Nexus peer node.
    pub fn on_peer_antibody(&mut self, payload: AntibodyPayload) -> bool {
        self.stats.antibodies_recv += 1;
        let is_new = self.generator.receive_from_peer(payload.clone());
        if is_new {
            self.registry.apply(payload, true);
        }
        is_new
    }

    /// Drain antibodies queued for broadcast to Nexus peers.
    pub fn drain_for_broadcast(&mut self) -> Vec<AntibodyPayload> {
        let payloads = self.generator.drain_broadcast_queue();
        self.stats.broadcasts_queued += payloads.len() as u64;
        payloads
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AntibodyBridge: checks={} blocked={} gen={} recv={} queued={}",
            self.stats.spawn_checks, self.stats.spawn_blocked,
            self.stats.antibodies_gen, self.stats.antibodies_recv, self.stats.broadcasts_queued
        );
    }
}
