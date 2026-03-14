//! # Firstboot Antibody Bridge (Phase 161)
//!
//! ## Architecture Guardian: The Gap
//! `digital_antibody.rs` implements:
//! - `LocalImmunityRegistry::apply(payload: AntibodyPayload, from_peer: bool)` → bool
//! - `LocalImmunityRegistry::is_binary_blacklisted(&[u8;32])` → bool
//! - `LocalImmunityRegistry::scan(observed_hash)` → Option<&AntibodyPayload>
//! - `AntibodyGenerator::new(node_id: u64)` — per-node generator
//!
//! `AntibodyPayload` fields: antibody_id, origin_node, qindows_version,
//! signature (BehaviouralSignature), action (AntibodyAction), generated_at,
//! ttl, signature_bytes: [u8;64], severity, description: String
//!
//! `AntibodyAction`: VaporizeSilo / QuarantineAndAlert / BlacklistBinary /
//! StripNetSend / ThrottleSilo / AlertOnly
//!
//! `ThreatCategory`: CapabilityEscalation / CovertChannel / MemoryCorruption /
//! BinaryTampering / DenialOfService / LateralMovement / ExfilAttempt / Unknown
//!
//! **Missing link**: `LocalImmunityRegistry` started empty — no boot-time
//! threat signatures were seeded. Silo spawns had no antibody baseline.
//!
//! This module provides `FirstbootAntibodyBridge`:
//! 1. `seed_known_threats()` — seeds known binary threat hashes
//! 2. `is_silo_spawn_safe()` — scan binary hash before spawn

extern crate alloc;
use alloc::string::String;

use crate::digital_antibody::{
    LocalImmunityRegistry, AntibodyGenerator, AntibodyPayload, BehaviouralSignature,
    ThreatCategory, AntibodyAction,
};

#[derive(Debug, Default, Clone)]
pub struct FirstbootAntibodyStats {
    pub threats_seeded: u64,
    pub spawn_checks:   u64,
    pub spawn_blocked:  u64,
}

pub struct FirstbootAntibodyBridge {
    pub registry:  LocalImmunityRegistry,
    pub generator: AntibodyGenerator,
    pub stats:     FirstbootAntibodyStats,
    next_id:       u64,
}

impl FirstbootAntibodyBridge {
    pub fn new(node_id: u64) -> Self {
        FirstbootAntibodyBridge {
            registry:  LocalImmunityRegistry::new(),
            generator: AntibodyGenerator::new(node_id),
            stats:     FirstbootAntibodyStats::default(),
            next_id:   1,
        }
    }

    /// Seed known-malicious binary hashes at first boot.
    pub fn seed_known_threats(&mut self, threat_hashes: &[[u8; 32]], tick: u64) {
        for &hash in threat_hashes {
            let sig = BehaviouralSignature::simple(ThreatCategory::BinaryTampering, hash);
            let payload = AntibodyPayload {
                antibody_id:      self.next_id,
                origin_node:      0,
                qindows_version:  1,
                signature:        sig,
                action:           AntibodyAction::BlacklistBinary,
                generated_at:     tick,
                ttl:              0,  // local-only, not propagated
                signature_bytes:  [0u8; 64],
                severity:         98,
                description:      String::from("boot-seeded malicious binary"),
            };
            self.next_id += 1;
            self.registry.apply(payload, false);
            self.stats.threats_seeded += 1;
        }
        crate::serial_println!(
            "[FIRSTBOOT ANTIBODY] {} threat signatures seeded", self.stats.threats_seeded
        );
    }

    /// Check if a Silo binary is safe to spawn.
    pub fn is_silo_spawn_safe(&mut self, binary_hash: &[u8; 32]) -> bool {
        self.stats.spawn_checks += 1;
        if self.registry.is_binary_blacklisted(binary_hash) {
            self.stats.spawn_blocked += 1;
            crate::serial_println!("[FIRSTBOOT ANTIBODY] Silo spawn BLOCKED (blacklisted binary)");
            return false;
        }
        if let Some(payload) = self.registry.scan(binary_hash) {
            if payload.action == AntibodyAction::BlacklistBinary
                || payload.action == AntibodyAction::VaporizeSilo
            {
                self.stats.spawn_blocked += 1;
                return false;
            }
        }
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FirstbootAntibody: seeded={} checks={} blocked={}",
            self.stats.threats_seeded, self.stats.spawn_checks, self.stats.spawn_blocked
        );
    }
}
