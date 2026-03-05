//! # Synapse Neural Encryption
//!
//! Hardware enclave filtering for Brain-Computer Interface (BCI)
//! signals. The OS never sees raw brainwaves — hardware enclaves
//! filter out private thoughts/memories, providing the Qernel
//! only with computed "Intent Hashes" (Section 6.2).
//!
//! Architecture:
//! 1. Raw BCI signal arrives via USB/Bluetooth neural headset
//! 2. Signal enters a **Secure Enclave** (TPM 3.0 / SGX-like)
//! 3. Enclave runs a local ML model to extract intent features
//! 4. Only the intent hash leaves the enclave — raw signal is wiped
//! 5. The Thought-Gate handshake prevents accidental command firing
//!
//! Privacy guarantee: No process, not even Ring 0, can see raw neural data.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Neural signal quality levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalQuality {
    /// Excellent (>90% SNR)
    Excellent,
    /// Good (70-90% SNR)
    Good,
    /// Fair (50-70% SNR)
    Fair,
    /// Poor (<50% SNR — may produce false positives)
    Poor,
    /// No signal / disconnected
    NoSignal,
}

/// An intent hash — the ONLY output from the neural enclave.
/// Raw brainwaves are never exposed outside the hardware boundary.
#[derive(Debug, Clone)]
pub struct IntentHash {
    /// 256-bit hash of the computed intent vector
    pub hash: [u8; 32],
    /// Confidence score (0.0 – 1.0)
    pub confidence: f32,
    /// Intent category
    pub category: IntentCategory,
    /// Timestamp (monotonic ticks)
    pub timestamp: u64,
    /// The enclave attestation nonce (proves origin)
    pub attestation_nonce: u64,
}

/// Categories of neural intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentCategory {
    /// Motor intent (move cursor, scroll, gesture)
    Motor,
    /// Selection intent (confirm, click, activate)
    Selection,
    /// Navigation intent (switch window, go back)
    Navigation,
    /// Text input intent (thought-to-text)
    TextInput,
    /// System command (open palette, lock, shutdown)
    SystemCommand,
    /// Emotional state (context hint, not a command)
    EmotionalContext,
    /// Unknown / noise
    Unknown,
}

/// Thought-Gate state — prevents accidental intent firing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThoughtGateState {
    /// Gate is closed — intents are buffered but not dispatched
    Closed,
    /// First "tap" detected — waiting for confirmation
    FirstTap,
    /// Gate open — intents are dispatched to the system
    Open,
    /// Cooldown after gate close (prevents re-trigger)
    Cooldown,
}

/// The Thought-Gate — "mental double-tap" handshake.
#[derive(Debug, Clone)]
pub struct ThoughtGate {
    /// Current state
    pub state: ThoughtGateState,
    /// Timestamp of first tap
    pub first_tap_time: u64,
    /// Maximum time between taps (ms)
    pub tap_window_ms: u64,
    /// Cooldown duration (ms)
    pub cooldown_ms: u64,
    /// Gate open duration before auto-close (ms)
    pub auto_close_ms: u64,
    /// Time the gate was opened
    pub opened_at: u64,
    /// Total opens
    pub total_opens: u64,
}

impl ThoughtGate {
    pub fn new() -> Self {
        ThoughtGate {
            state: ThoughtGateState::Closed,
            first_tap_time: 0,
            tap_window_ms: 500,    // 500ms window for double-tap
            cooldown_ms: 200,      // 200ms cooldown
            auto_close_ms: 10_000, // 10 second auto-close
            opened_at: 0,
            total_opens: 0,
        }
    }

    /// Process a tap signal from the enclave.
    pub fn on_tap(&mut self, now: u64) -> bool {
        match self.state {
            ThoughtGateState::Closed => {
                self.state = ThoughtGateState::FirstTap;
                self.first_tap_time = now;
                false
            }
            ThoughtGateState::FirstTap => {
                let elapsed = now.saturating_sub(self.first_tap_time);
                if elapsed <= self.tap_window_ms {
                    // Double-tap confirmed — open gate
                    self.state = ThoughtGateState::Open;
                    self.opened_at = now;
                    self.total_opens += 1;
                    true
                } else {
                    // Too slow — reset
                    self.state = ThoughtGateState::FirstTap;
                    self.first_tap_time = now;
                    false
                }
            }
            ThoughtGateState::Open => {
                // Tap while open = close gate
                self.state = ThoughtGateState::Cooldown;
                false
            }
            ThoughtGateState::Cooldown => false,
        }
    }

    /// Tick the gate (check timeouts).
    pub fn tick(&mut self, now: u64) {
        match self.state {
            ThoughtGateState::FirstTap => {
                if now.saturating_sub(self.first_tap_time) > self.tap_window_ms {
                    self.state = ThoughtGateState::Closed;
                }
            }
            ThoughtGateState::Open => {
                if now.saturating_sub(self.opened_at) > self.auto_close_ms {
                    self.state = ThoughtGateState::Cooldown;
                }
            }
            ThoughtGateState::Cooldown => {
                if now.saturating_sub(self.opened_at) > self.auto_close_ms + self.cooldown_ms {
                    self.state = ThoughtGateState::Closed;
                }
            }
            _ => {}
        }
    }

    pub fn is_open(&self) -> bool {
        self.state == ThoughtGateState::Open
    }
}

/// Secure neural enclave (hardware-backed black box).
pub struct NeuralEnclave {
    /// Is the enclave attestation valid?
    pub attested: bool,
    /// Enclave measurement hash (hardware identity)
    pub measurement: [u8; 32],
    /// Monotonic nonce counter
    nonce_counter: u64,
    /// Signal quality
    pub quality: SignalQuality,
    /// Discarded raw samples (privacy metric)
    pub samples_discarded: u64,
}

impl NeuralEnclave {
    pub fn new() -> Self {
        NeuralEnclave {
            attested: false,
            measurement: [0; 32],
            nonce_counter: 1,
            quality: SignalQuality::NoSignal,
            samples_discarded: 0,
        }
    }

    /// Attest the enclave (verify hardware integrity).
    pub fn attest(&mut self, measurement: [u8; 32]) -> bool {
        // In production: verify via TPM 3.0 PCR attestation
        if measurement != [0; 32] {
            self.measurement = measurement;
            self.attested = true;
            true
        } else {
            false
        }
    }

    /// Process raw neural data INSIDE the enclave.
    /// Returns only the intent hash — raw data never leaves.
    pub fn process_signal(
        &mut self,
        raw_samples: &[f32],
        now: u64,
    ) -> Option<IntentHash> {
        if !self.attested {
            return None;
        }

        // Update signal quality based on sample variance
        self.quality = if raw_samples.is_empty() {
            SignalQuality::NoSignal
        } else {
            let mean: f32 = raw_samples.iter().sum::<f32>() / raw_samples.len() as f32;
            let variance: f32 = raw_samples.iter()
                .map(|x| (x - mean) * (x - mean))
                .sum::<f32>() / raw_samples.len() as f32;
            let snr = if variance > 0.0 { mean.abs() / variance.sqrt() } else { 0.0 };

            if snr > 3.0 { SignalQuality::Excellent }
            else if snr > 2.0 { SignalQuality::Good }
            else if snr > 1.0 { SignalQuality::Fair }
            else { SignalQuality::Poor }
        };

        // Compute intent features (simplified ML inference)
        // In production: run a quantized neural network on the NPU
        let mut feature_hash = [0u8; 32];
        for (i, &sample) in raw_samples.iter().enumerate().take(32) {
            feature_hash[i % 32] ^= (sample.to_bits() >> 16) as u8;
            feature_hash[(i + 7) % 32] = feature_hash[(i + 7) % 32]
                .wrapping_add((sample.to_bits() & 0xFF) as u8);
        }

        // Classify intent category from features
        let category = self.classify_intent(&feature_hash);
        let confidence = match self.quality {
            SignalQuality::Excellent => 0.95,
            SignalQuality::Good => 0.80,
            SignalQuality::Fair => 0.60,
            SignalQuality::Poor => 0.30,
            SignalQuality::NoSignal => return None,
        };

        let nonce = self.nonce_counter;
        self.nonce_counter += 1;

        // Raw samples are discarded — never leave the enclave
        self.samples_discarded = self.samples_discarded
            .saturating_add(raw_samples.len() as u64);

        Some(IntentHash {
            hash: feature_hash,
            confidence,
            category,
            timestamp: now,
            attestation_nonce: nonce,
        })
    }

    /// Classify intent category from feature hash.
    fn classify_intent(&self, features: &[u8; 32]) -> IntentCategory {
        // Simplified: use first byte as category selector
        let dominant = features[0];
        match dominant % 7 {
            0 => IntentCategory::Motor,
            1 => IntentCategory::Selection,
            2 => IntentCategory::Navigation,
            3 => IntentCategory::TextInput,
            4 => IntentCategory::SystemCommand,
            5 => IntentCategory::EmotionalContext,
            _ => IntentCategory::Unknown,
        }
    }
}

/// The Neural Encryption Manager.
pub struct NeuralEncryption {
    /// The secure enclave
    pub enclave: NeuralEnclave,
    /// The Thought Gate
    pub gate: ThoughtGate,
    /// Buffered intents (pending gate open)
    pub intent_buffer: Vec<IntentHash>,
    /// Dispatched intents
    pub dispatched: Vec<IntentHash>,
    /// Minimum confidence to dispatch
    pub min_confidence: f32,
    /// Statistics
    pub stats: NeuralStats,
}

/// Neural pipeline statistics.
#[derive(Debug, Clone, Default)]
pub struct NeuralStats {
    pub signals_processed: u64,
    pub intents_generated: u64,
    pub intents_dispatched: u64,
    pub intents_filtered: u64,
    pub gate_opens: u64,
}

impl NeuralEncryption {
    pub fn new() -> Self {
        NeuralEncryption {
            enclave: NeuralEnclave::new(),
            gate: ThoughtGate::new(),
            intent_buffer: Vec::new(),
            dispatched: Vec::new(),
            min_confidence: 0.5,
            stats: NeuralStats::default(),
        }
    }

    /// Process raw BCI signal and optionally dispatch intent.
    pub fn process(&mut self, raw_samples: &[f32], now: u64) -> Option<IntentHash> {
        self.stats.signals_processed += 1;
        self.gate.tick(now);

        let intent = self.enclave.process_signal(raw_samples, now)?;
        self.stats.intents_generated += 1;

        if intent.confidence < self.min_confidence {
            self.stats.intents_filtered += 1;
            return None;
        }

        if self.gate.is_open() {
            self.stats.intents_dispatched += 1;
            self.dispatched.push(intent.clone());
            Some(intent)
        } else {
            // Buffer until gate opens
            self.intent_buffer.push(intent);
            None
        }
    }

    /// Drain dispatched intents.
    pub fn drain_dispatched(&mut self) -> Vec<IntentHash> {
        core::mem::take(&mut self.dispatched)
    }
}
