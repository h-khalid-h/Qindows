//! # Synapse Neural Gate Bridge (Phase 186)
//!
//! ## Architecture Guardian: The Gap
//! `synapse.rs` implements Q-Synapse BCI integration:
//! - `ThoughtGateState::update(pattern: NeuralPattern, tick)` → bool
//! - `NeuralBinding` — binding between neural pattern and Silo capability
//! - `IntentCategory` — categories of neural intent (Command, Navigation, etc.)
//!
//! **Missing link**: Synapse:READ/WRITE caps were defined but never checked
//! before processing neural input stream events. Any Silo could read from
//! the neural input buffer without authorization.
//!
//! This module provides `SynapseNeuralGateBridge`:
//! Neural stream access requires Synapse:EXEC cap check.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::synapse::{ThoughtGateState, NeuralPattern, IntentCategory};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct SynapseGateStats {
    pub events_allowed: u64,
    pub events_denied:  u64,
    pub gate_opens:     u64,
}

pub struct SynapseNeuralGateBridge {
    gates:     BTreeMap<u64, ThoughtGateState>,
    pub stats: SynapseGateStats,
}

impl SynapseNeuralGateBridge {
    pub fn new() -> Self {
        SynapseNeuralGateBridge { gates: BTreeMap::new(), stats: SynapseGateStats::default() }
    }

    /// Process a neural intent event — requires Synapse:READ cap.
    pub fn process_neural_event(
        &mut self,
        silo_id: u64,
        pattern: NeuralPattern,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Synapse, CAP_READ, 0, tick) {
            self.stats.events_denied += 1;
            return false;
        }
        self.stats.events_allowed += 1;
        let gate = self.gates.entry(silo_id).or_insert(ThoughtGateState::new());
        if gate.update(pattern, tick) {
            self.stats.gate_opens += 1;
            crate::serial_println!("[SYNAPSE] Silo {} neural gate OPEN", silo_id);
            true
        } else {
            false
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SynapseGateBridge: allowed={} denied={} opens={}",
            self.stats.events_allowed, self.stats.events_denied, self.stats.gate_opens
        );
    }
}
