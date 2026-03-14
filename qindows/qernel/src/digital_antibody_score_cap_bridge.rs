#![no_std]

use crate::digital_antibody::AntibodyPayload;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ};

/// Bridge for Phase 298: Digital Antibody Threat Score Cap Bridge
/// Re-verifies `Sentinel:READ` capability before allowing a Silo to query the threat intelligence score.
pub struct DigitalAntibodyScoreCapBridge<'a> {
    target: &'a AntibodyPayload,
}

impl<'a> DigitalAntibodyScoreCapBridge<'a> {
    pub fn new(target: &'a AntibodyPayload) -> Self {
        Self { target }
    }

    pub fn match_score(
        &self,
        silo_id: u64,
        observed_hash: &[u8; 32],
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u8> {
        if !forge.check(silo_id, CapType::Sentinel, CAP_READ, 0, tick) {
            crate::serial_println!(
                "[DIGITAL ANTIBODY] Silo {} threat score query denied — Sentinel:READ required", silo_id
            );
            return None;
        }

        Some(self.target.signature.match_score(observed_hash))
    }
}
