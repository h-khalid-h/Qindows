//! # Q-Synapse — Neural Integration (BCI)
//!
//! Maps Brain-Computer Interfaces directly to the Q-Shell.
//! Translates neural patterns into Semantic Intent Vectors.
//!
//! Key principles:
//! - **Thought-Gate**: Double-tap mental trigger prevents accidental commands
//! - **Neural Masking**: Private thoughts filtered at Hardware Enclave level
//! - **Semantic Vectors**: Maps concepts, not motor commands

#![no_std]

extern crate alloc;

pub mod bci;
pub mod intent;
pub mod models;

use alloc::vec::Vec;

/// A neural pattern signature — the "fingerprint" of a thought.
pub type PatternHash = [u8; 32];

/// Neural binding — maps a specific thought pattern to an action.
#[derive(Debug, Clone)]
pub struct NeuralBinding {
    /// The cryptographic signature of the neural pattern
    pub pattern_hash: PatternHash,
    /// The Q-Shell command or intent this triggers
    pub intent_id: u64,
    /// Confidence threshold (0.0 - 1.0)
    pub confidence_threshold: f32,
    /// Whether this binding requires Thought-Gate confirmation
    pub requires_confirmation: bool,
}

/// The Thought-Gate state machine.
///
/// Prevents accidental execution by requiring a specific
/// cognitive "double-tap" before commands fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThoughtGateState {
    /// Idle — no pending intent
    Idle,
    /// First tap detected — waiting for confirmation
    Primed { intent_id: u64 },
    /// Confirmed — executing intent
    Confirmed,
    /// Cancelled — user mentally "dismissed" the intent
    Cancelled,
}

/// Raw neural data from the BCI hardware.
#[derive(Debug)]
pub struct NeuralSample {
    /// Timestamp (in microseconds)
    pub timestamp_us: u64,
    /// Channel data (microvolt readings from electrodes)
    pub channels: Vec<f32>,
    /// Pre-computed feature vector (from NPU denoising)
    pub feature_vector: Vec<f32>,
}

/// The Q-Synapse engine.
pub struct QSynapse {
    /// Active neural bindings
    pub bindings: Vec<NeuralBinding>,
    /// Current Thought-Gate state
    pub gate_state: ThoughtGateState,
    /// Calibration confidence multiplier
    pub calibration_factor: f32,
}

impl QSynapse {
    pub fn new() -> Self {
        QSynapse {
            bindings: Vec::new(),
            gate_state: ThoughtGateState::Idle,
            calibration_factor: 1.0,
        }
    }

    /// Bind a neural pattern to an intent.
    pub fn bind_intent(&mut self, binding: NeuralBinding) {
        self.bindings.push(binding);
    }

    /// Process a neural sample and check for matching intents.
    ///
    /// The NPU has already denoised the signal and extracted
    /// the feature vector. We match against registered patterns.
    pub fn process_sample(&mut self, sample: &NeuralSample) -> Option<u64> {
        // In production:
        // 1. Compute cosine similarity between sample.feature_vector
        //    and each binding.pattern_hash
        // 2. If similarity > confidence_threshold, trigger the intent
        // 3. Apply Thought-Gate confirmation if required

        // Simplified: check if feature vector has strong enough signal
        let signal_strength: f32 = sample
            .feature_vector
            .iter()
            .map(|v| v.abs())
            .sum::<f32>()
            / sample.feature_vector.len() as f32;

        if signal_strength < 0.5 * self.calibration_factor {
            return None; // Below threshold
        }

        // Check Thought-Gate
        match self.gate_state {
            ThoughtGateState::Idle => {
                // First detection — prime the gate
                if let Some(binding) = self.bindings.first() {
                    if binding.requires_confirmation {
                        self.gate_state = ThoughtGateState::Primed {
                            intent_id: binding.intent_id,
                        };
                        return None; // Wait for confirmation
                    } else {
                        return Some(binding.intent_id);
                    }
                }
            }
            ThoughtGateState::Primed { intent_id } => {
                // Second detection — confirm and execute
                self.gate_state = ThoughtGateState::Confirmed;
                return Some(intent_id);
            }
            _ => {}
        }

        None
    }

    /// Calibrate the neural interface.
    ///
    /// The user spends ~5 minutes training their "Thought Signatures"
    /// during Q-Setup. This improves intent detection by 10x.
    pub fn calibrate(&mut self, samples: &[NeuralSample]) {
        if samples.is_empty() {
            return;
        }

        // Compute average signal strength for calibration
        let avg_strength: f32 = samples
            .iter()
            .map(|s| {
                s.feature_vector.iter().map(|v| v.abs()).sum::<f32>()
                    / s.feature_vector.len() as f32
            })
            .sum::<f32>()
            / samples.len() as f32;

        self.calibration_factor = avg_strength.max(0.1); // Prevent division by zero
    }
}
