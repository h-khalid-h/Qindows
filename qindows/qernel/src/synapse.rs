//! # Q-Synapse — Neural Intent Interface (Phase 60)
//!
//! Q-Synapse is the Qindows BCI (Brain-Computer Interface) subsystem.
//! It bridges high-bandwidth neural signal streams from EEG/implant hardware
//! into typed, executable *Intent Vectors* consumed by the Q-Kernel.
//!
//! ## Architecture (from ARCHITECTURE.md §6)
//! ```text
//!   BCI Hardware (EEG / Implant)
//!        │  raw microvolt stream (Q-Ring: SynapseSubmit)
//!        ▼
//!   SignalPipeline: denoise → embed → classify
//!        │  NeuralPattern (256-bit hash + confidence)
//!        ▼
//!   NeuralBindingTable: pattern_hash → IntentAction
//!        │  matched binding
//!        ▼
//!   ThoughtGate: double-tap mental handshake required
//!        │  confirmed intent
//!        ▼
//!   Q-Shell / Aether executes the action
//! ```
//!
//! ## Q-Manifest Law 6: Silo Sandbox + Privacy
//! Raw neural data NEVER leaves the `SynapseProcessor`. Only the
//! **Intent Hash** (a structured, de-personalized semantic vector) reaches
//! any other kernel component. Private thoughts are filtered at the
//! hardware enclave level before this module even receives them.
//!
//! ## Architecture Guardian: Single Responsibility
//! This module owns ONE thing: neural signal → intent translation.
//! It does NOT schedule fibers, draw windows, or access the file system.
//! It emits `IntentEvent`s consumed by Q-Shell via the Q-Ring.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── Neural Signal Types ───────────────────────────────────────────────────────

/// Maximum number of neural bindings per Silo (prevents resource exhaustion).
pub const MAX_BINDINGS_PER_SILO: usize = 128;

/// The minimum confidence score to act on a neural pattern (0.0–1.0).
pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.82;

/// A raw neural pattern captured from the BCI hardware.
/// Represented as a 256-bit semantic hash (16 bytes) produced by the
/// on-chip NPU inference model after denoising and dimensional reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NeuralPattern(pub [u8; 16]);

impl NeuralPattern {
    pub const IDLE: Self = NeuralPattern([0u8; 16]);
}

/// Confidence score for a neural pattern match [0.0, 1.0].
pub type Confidence = f32;

/// Semantic intent categories recognized by Q-Synapse.
/// Maps directly to Q-Shell actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentCategory {
    /// General navigation / spatial movement
    Navigate,
    /// Open / bring-forward a specific context
    Focus,
    /// Confirm / execute the currently highlighted action
    Execute,
    /// Cancel or dismiss the current context
    Dismiss,
    /// Switch workspace / context layer
    Pivot,
    /// Trigger the Q-Shell command palette
    OpenShell,
    /// Emergency stop — freeze all pending actions
    Abort,
    /// Custom user-defined intent (bound via `SynapseConfirm`)
    Custom(u32),
}

/// The resolved intent event emitted after ThoughtGate confirmation.
#[derive(Debug, Clone)]
pub struct IntentEvent {
    /// Which Silo generated this intent
    pub silo_id: u64,
    /// The matched neural pattern
    pub pattern: NeuralPattern,
    /// Intent category
    pub category: IntentCategory,
    /// Confidence score of the match
    pub confidence: Confidence,
    /// Kernel tick when the intent was confirmed
    pub timestamp: u64,
    /// Was the HandshakeGate double-tap verified?
    pub gate_confirmed: bool,
}

// ── Neural Binding Table ──────────────────────────────────────────────────────

/// A single binding: neural pattern → intent action.
///
/// Created by `SyscallId::SynapseConfirm` — requires the user to
/// perform the cognitive gesture twice within a 2-second window.
#[derive(Debug, Clone)]
pub struct NeuralBinding {
    /// The hash of the recorded neural pattern.
    pub pattern: NeuralPattern,
    /// The intent category this pattern fires.
    pub category: IntentCategory,
    /// Confidence threshold specific to this binding (overrides default).
    pub threshold: Confidence,
    /// Silo that owns this binding.
    pub owner_silo: u64,
    /// Times this binding has been successfully matched.
    pub match_count: u64,
    /// Is this binding currently enabled?
    pub enabled: bool,
}

// ── ThoughtGate — Double-Tap Handshake ───────────────────────────────────────

/// ThoughtGate prevents accidental neural intent execution.
///
/// A pattern must match TWICE within `GATE_WINDOW_TICKS` for the
/// intent to be confirmed and emitted. This mirrors the "cognitive
/// double-tap" described in ARCHITECTURE.md §6.
pub const GATE_WINDOW_TICKS: u64 = 2000; // ~2 seconds at 1kHz

#[derive(Debug, Clone)]
pub struct ThoughtGateState {
    /// Pattern seen in the first half of the handshake
    pub pending_pattern: Option<NeuralPattern>,
    /// Kernel tick when pending_pattern was recorded
    pub pending_since: u64,
}

impl ThoughtGateState {
    pub fn new() -> Self {
        ThoughtGateState { pending_pattern: None, pending_since: 0 }
    }

    /// Feed a new neural match into the gate. Returns true if confirmed.
    pub fn update(&mut self, pattern: NeuralPattern, tick: u64) -> bool {
        match self.pending_pattern {
            Some(prev) if prev == pattern => {
                // Second match — check time window
                let elapsed = tick.saturating_sub(self.pending_since);
                self.pending_pattern = None;
                elapsed <= GATE_WINDOW_TICKS
            }
            _ => {
                // First match — prime the gate
                self.pending_pattern = Some(pattern);
                self.pending_since = tick;
                false
            }
        }
    }
}

// ── Signal Pipeline ───────────────────────────────────────────────────────────

/// Simulated BCI signal denoising and classification result.
#[derive(Debug, Clone)]
pub struct ClassifiedSignal {
    pub pattern: NeuralPattern,
    pub confidence: Confidence,
}

/// Denoise + embed + classify a raw neural sample.
///
/// In production: runs a quantized transformer on the NPU.
/// Here: a deterministic hash-based placeholder that demonstrates
/// the pipeline structure without actual neural data.
pub fn classify_neural_sample(raw_bytes: &[u8]) -> Option<ClassifiedSignal> {
    if raw_bytes.is_empty() { return None; }

    // Deterministic NPU simulation: FNV-hash the sample data
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in raw_bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }

    let mut pattern_bytes = [0u8; 16];
    pattern_bytes[..8].copy_from_slice(&h.to_le_bytes());
    pattern_bytes[8..].copy_from_slice(&(h ^ 0xDEAD_BEEF_CAFE_0000).to_le_bytes());

    // Confidence derived from signal entropy (placeholder: fixed 0.85)
    let confidence: f32 = 0.85;

    Some(ClassifiedSignal {
        pattern: NeuralPattern(pattern_bytes),
        confidence,
    })
}

// ── Synapse Processor ─────────────────────────────────────────────────────────

/// Statistics for Q-Synapse monitoring.
#[derive(Debug, Default, Clone)]
pub struct SynapseStats {
    pub samples_processed: u64,
    pub patterns_classified: u64,
    pub bindings_matched: u64,
    pub gate_confirmations: u64,
    pub gate_timeouts: u64,
    pub low_confidence_dropped: u64,
}

/// The central Q-Synapse processor.
///
/// Owns the per-Silo binding tables, the ThoughtGate state, and
/// the confirmed intent event queue.
pub struct SynapseProcessor {
    /// Silo ID → binding list
    pub bindings: BTreeMap<u64, Vec<NeuralBinding>>,
    /// Per-Silo ThoughtGate state
    pub gates: BTreeMap<u64, ThoughtGateState>,
    /// Queued confirmed intents waiting for Q-Shell consumption
    pub intent_queue: Vec<IntentEvent>,
    /// Global confidence threshold (per-binding can override)
    pub default_threshold: Confidence,
    /// Stats
    pub stats: SynapseStats,
}

impl SynapseProcessor {
    pub fn new() -> Self {
        SynapseProcessor {
            bindings: BTreeMap::new(),
            gates: BTreeMap::new(),
            intent_queue: Vec::new(),
            default_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
            stats: SynapseStats::default(),
        }
    }

    /// Register a neural binding for a Silo.
    ///
    /// Called by `SyscallId::SynapseConfirm` after the user performs
    /// the cognitive double-tap to confirm their intent pattern.
    pub fn register_binding(
        &mut self,
        silo_id: u64,
        binding: NeuralBinding,
    ) -> Result<(), &'static str> {
        let list = self.bindings.entry(silo_id).or_default();
        if list.len() >= MAX_BINDINGS_PER_SILO {
            return Err("Q-Synapse: binding quota exceeded for Silo");
        }
        crate::serial_println!(
            "[SYNAPSE] Binding registered: Silo {} pattern {:?} → {:?}",
            silo_id, binding.pattern.0, binding.category
        );
        list.push(binding);
        Ok(())
    }

    /// Process a raw neural sample from the BCI hardware.
    ///
    /// Called by the `SynapseSubmit` syscall handler.
    /// The full pipeline: classify → match → gate → emit.
    pub fn process_sample(
        &mut self,
        silo_id: u64,
        raw_bytes: &[u8],
        tick: u64,
    ) {
        self.stats.samples_processed += 1;

        // 1. Classify
        let signal = match classify_neural_sample(raw_bytes) {
            Some(s) => s,
            None => return,
        };
        self.stats.patterns_classified += 1;

        // 2. Match against this Silo's bindings
        let matched = self.bindings.get(&silo_id).and_then(|list| {
            list.iter().find(|b| {
                b.enabled
                    && b.pattern == signal.pattern
                    && signal.confidence >= b.threshold
            }).cloned()
        });

        let binding = match matched {
            Some(b) => b,
            None => {
                if signal.confidence < self.default_threshold {
                    self.stats.low_confidence_dropped += 1;
                }
                return;
            }
        };
        self.stats.bindings_matched += 1;

        // 3. ThoughtGate double-tap check
        let gate = self.gates.entry(silo_id).or_insert_with(ThoughtGateState::new);
        let confirmed = gate.update(signal.pattern, tick);

        if !confirmed {
            crate::serial_println!(
                "[SYNAPSE] Pattern {:?} primed (awaiting double-tap) for Silo {}",
                signal.pattern.0, silo_id
            );
            return;
        }
        self.stats.gate_confirmations += 1;

        // 4. Emit confirmed IntentEvent
        crate::serial_println!(
            "[SYNAPSE] Intent CONFIRMED: Silo {} → {:?} (confidence={:.2})",
            silo_id, binding.category, signal.confidence
        );

        self.intent_queue.push(IntentEvent {
            silo_id,
            pattern: signal.pattern,
            category: binding.category,
            confidence: signal.confidence,
            timestamp: tick,
            gate_confirmed: true,
        });
    }

    /// Dequeue the next confirmed intent for a Silo (consumed by Q-Shell).
    pub fn dequeue_intent(&mut self, silo_id: u64) -> Option<IntentEvent> {
        let pos = self.intent_queue.iter().position(|e| e.silo_id == silo_id)?;
        Some(self.intent_queue.remove(pos))
    }

    /// Remove all bindings for a Silo (called on vaporize).
    pub fn remove_silo_bindings(&mut self, silo_id: u64) {
        self.bindings.remove(&silo_id);
        self.gates.remove(&silo_id);
        crate::serial_println!("[SYNAPSE] Silo {} bindings cleared (vaporize).", silo_id);
    }
}
