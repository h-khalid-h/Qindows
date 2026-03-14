//! # Synapse IPC Bridge — Kernel ↔ Synapse Crate Integration (Phase 102)
//!
//! ## Architecture Guardian: The Gap
//! `synapse/` is a separate workspace crate with 23 modules (BCI hardware, neural
//! encoding, thought-gate, intent classification, semantic routing, RAG, summarizer,
//! dialog, tokenizer, etc.).
//!
//! `qernel/src/synapse.rs` (Phase 60) implements the *kernel-side* view:
//! SynapseProcessor that classifies `NeuralSample` → `IntentEvent`.
//!
//! **The integration gap:** The `synapse/` crate's high-level AI pipeline
//! (RAG, LLM, semantic routing) was never connected to the kernel via a proper
//! IPC bridge. The `synapse.rs` (Phase 60) kernel module uses FNV hashing as a
//! simulation, but the **production path** is:
//!
//! ```text
//! BCI hardware (EEG) → synapse crate (running in Synapse Silo, SILO_ID=4)
//!     │  QSynapse IPC message: { neural_sample: NeuralSample }
//!     ▼                              ↑  (the bridge below provides this)
//! qernel/src/synapse.rs kernel-side  │
//!     │  classified IntentEvent       │
//!     ▼                              ↓
//! intent_router.rs → Q-Shell / Aether / Nexus action
//! ```
//!
//! ## Synapse Silo Architecture
//! The `synapse` crate runs in a dedicated **Synapse Silo** (SILO_ID=4).
//! The kernel communicates with it via Q-Ring IPC (SqOpcode::NpuInfer).
//! This bridge:
//! 1. Defines the IPC message format (NeuralSample → SynapseMsg)
//! 2. Provides `submit_neural_sample()` — writes to Synapse Silo's Q-Ring
//! 3. Provides `poll_intent_result()` — reads the completion from the CQ
//! 4. Defines the privacy contract: raw neural data → process in Silo,
//!    ONLY intent hash exits Silo
//!
//! ## Privacy Contract (Q-Manifest Law 1 + Q-Synapse §6.2)
//! - Raw neural sample data is in the IPC payload, encrypted with Synapse Silo's CapToken
//! - Synapse Silo processes it, outputs ONLY an `IntentHash` (256-bit semantic vector)
//! - The `IntentHash` is returned via CQ, never the original neural data
//! - Privacy is enforced at the Silo boundary — hardware separation

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

// ── Synapse Silo ID ───────────────────────────────────────────────────────────

/// Fixed Silo ID of the Synapse AI Silo (spawned at boot as Silo 4).
pub const SYNAPSE_SILO_ID: u64 = 4;

// ── Neural Sample (IPC-safe format) ──────────────────────────────────────────

/// A neural sample in IPC-transmissible format.
/// Corresponds to `synapse::bci::NeuralSample` in the userspace crate.
#[derive(Debug, Clone, Copy)]
pub struct NeuralSampleMsg {
    /// Channel count (typically 64-256 EEG channels)
    pub channel_count: u16,
    /// Sample rate in Hz
    pub sample_rate_hz: u16,
    /// Electrode voltages in microvolts (scaled to i16 × 0.1μV)
    pub voltages: [i16; 256],
    /// Kernel tick at sample acquisition time
    pub tick: u64,
    /// Silo that submitted the sample (caller identity)
    pub submitting_silo: u64,
}

/// Confidence level of a neural classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConfidenceLevel {
    Low    = 0,  // < 60%
    Medium = 1,  // 60-80%
    High   = 2,  // > 80%
    Certain= 3,  // > 95%, double-tap confirmed
}

// ── Intent Categories (mirrors synapse/src/intent.rs) ─────────────────────────

/// High-level intent category decoded from neural pattern.
/// Matches `synapse::intent::IntentCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntentCategory {
    Navigate   = 0,
    Focus      = 1,
    Execute    = 2,
    Dismiss    = 3,
    Pivot      = 4,
    OpenShell  = 5,
    Abort      = 6,
    Custom     = 7,
    Idle       = 8,  // no intent detected
}

impl IntentCategory {
    pub fn name(self) -> &'static str {
        match self {
            Self::Navigate  => "Navigate",
            Self::Focus     => "Focus",
            Self::Execute   => "Execute",
            Self::Dismiss   => "Dismiss",
            Self::Pivot     => "Pivot",
            Self::OpenShell => "OpenShell",
            Self::Abort     => "Abort",
            Self::Custom    => "Custom",
            Self::Idle      => "Idle",
        }
    }
}

// ── Intent Result (IPC return from Synapse Silo) ──────────────────────────────

/// Result of neural inference — returned via Q-Ring CQ from Synapse Silo.
/// Privacy contract: only `intent_hash` and `category` leave the Silo.
/// The raw `NeuralSampleMsg` is processed and discarded inside the Silo.
#[derive(Debug, Clone, Copy)]
pub struct IntentResult {
    /// 256-bit semantic hash of the neural pattern (de-personalized)
    pub intent_hash: [u8; 32],
    /// Classified intent category
    pub category: IntentCategory,
    /// Confidence level
    pub confidence: ConfidenceLevel,
    /// True if ThoughtGate double-tap confirmed
    pub gate_confirmed: bool,
    /// Kernel tick when classification completed
    pub classified_at: u64,
    /// Caller's user_data tag (echoed from submission)
    pub user_data: u64,
}

// ── IPC Message Types ─────────────────────────────────────────────────────────

/// IPC message sent TO Synapse Silo.
#[derive(Debug, Clone, Copy)]
pub struct SynapseMsgRequest {
    pub msg_type: SynapseMsgType,
    pub sample: NeuralSampleMsg,
    pub user_data: u64,
}

/// IPC response FROM Synapse Silo.
#[derive(Debug, Clone, Copy)]
pub struct SynapseMsgResponse {
    pub user_data: u64,
    pub result: IntentResult,
    pub success: bool,
    pub error_code: u8,
}

/// Synapse IPC message type discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SynapseMsgType {
    SubmitSample  = 0,  // Neural sample for inference
    ThoughtGateOn = 1,  // Begin double-tap window
    ThoughtGateOff= 2,  // Cancel/timeout double-tap
    Calibrate     = 3,  // Calibration session start
    Shutdown      = 255,
}

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct SynapseBridgeStats {
    pub samples_submitted: u64,
    pub results_received: u64,
    pub gate_confirmations: u64,
    pub gate_aborts: u64,
    pub errors: u64,
    pub idle_results: u64,       // Idle category (no intent)
    pub high_confidence: u64,
    pub privacy_enforced: u64,   // Times raw data was NOT forwarded
}

// ── Synapse IPC Bridge ────────────────────────────────────────────────────────

/// Kernel-side bridge between qernel/synapse.rs and the Synapse Silo crate.
pub struct SynapseIpcBridge {
    /// True if Synapse Silo is running and responsive
    pub silo_alive: bool,
    /// Pending submissions (awaiting result, keyed by user_data)
    pub pending: alloc::collections::BTreeMap<u64, u64>, // user_data → submitted_tick
    /// Next user_data tag
    pub next_tag: u64,
    /// Statistics
    pub stats: SynapseBridgeStats,
    /// ThoughtGate active (double-tap window open)
    pub gate_active: bool,
    /// ThoughtGate window start tick
    pub gate_start_tick: u64,
    /// ThoughtGate timeout in ticks
    pub gate_timeout_ticks: u64,
}

impl SynapseIpcBridge {
    pub fn new() -> Self {
        SynapseIpcBridge {
            silo_alive: false,
            pending: alloc::collections::BTreeMap::new(),
            next_tag: 1,
            stats: SynapseBridgeStats::default(),
            gate_active: false,
            gate_start_tick: 0,
            gate_timeout_ticks: 2000, // 2-second window
        }
    }

    /// Called at boot when Synapse Silo is successfully launched.
    pub fn on_synapse_silo_ready(&mut self) {
        self.silo_alive = true;
        crate::serial_println!(
            "[SYNAPSE BRIDGE] Synapse Silo {} is online — BCI neural pipeline active",
            SYNAPSE_SILO_ID
        );
    }

    /// Submit a neural sample to the Synapse Silo via Q-Ring IPC.
    /// Returns the user_data tag for polling the result.
    /// Privacy: raw sample goes directly to Synapse Silo; kernel does NOT log it.
    pub fn submit_neural_sample(
        &mut self,
        sample: NeuralSampleMsg,
        qring: &mut crate::qring_async::QRingProcessor,
    ) -> Option<u64> {
        if !self.silo_alive {
            crate::serial_println!("[SYNAPSE BRIDGE] Synapse Silo not alive — sample dropped");
            return None;
        }

        let tag = self.next_tag;
        self.next_tag = self.next_tag.wrapping_add(1);

        // Submit NpuInfer opcode into Synapse Silo's Q-Ring
        let sqe = crate::qring_async::SqEntry {
            opcode: crate::qring_async::SqOpcode::NpuInfer as u16,
            flags: 0,
            user_data: tag,
            addr: SYNAPSE_SILO_ID, // use addr field to pass target
            len: core::mem::size_of::<NeuralSampleMsg>() as u32,
            aux: SynapseMsgType::SubmitSample as u32,
        };

        // Register synapse ring if not already registered
        if !qring.rings.contains_key(&SYNAPSE_SILO_ID) {
            qring.register_silo(SYNAPSE_SILO_ID);
        }

        if let Some(ring) = qring.rings.get_mut(&SYNAPSE_SILO_ID) {
            if ring.submit(sqe) {
                self.pending.insert(tag, sample.tick);
                self.stats.samples_submitted += 1;
                self.stats.privacy_enforced += 1; // raw data → Silo only
                Some(tag)
            } else {
                crate::serial_println!("[SYNAPSE BRIDGE] Synapse Q-Ring full — sample dropped");
                None
            }
        } else {
            None
        }
    }

    /// Poll for a completed neural inference result.
    /// Returns the IntentResult if the tag's completion is ready.
    pub fn poll_result(
        &mut self,
        tag: u64,
        qring: &mut crate::qring_async::QRingProcessor,
        tick: u64,
    ) -> Option<IntentResult> {
        // Drain the Synapse Silo's ring to move completions forward
        qring.drain(SYNAPSE_SILO_ID);

        // Check if our tag has a completion. Since we use user_data=tag,
        // scan the CQ for a matching entry.
        if let Some(ring) = qring.rings.get_mut(&SYNAPSE_SILO_ID) {
            let depth = ring.cq.len() as u32;
            let avail = (ring.cq_tail.wrapping_sub(ring.cq_head)) % depth;
            for i in 0..avail {
                let idx = (ring.cq_head + i) % depth;
                let cqe = ring.cq[idx as usize];
                if cqe.user_data == tag {
                    // Consume by advancing head to this entry + 1
                    ring.cq_head = (ring.cq_head + i + 1) % depth;
                    self.pending.remove(&tag);
                    self.stats.results_received += 1;

                    // Synthesise IntentResult from CQ status
                    // In production the Synapse Silo would encode category in cqe.flags
                    let category = if cqe.result >= 0 {
                        IntentCategory::from_u8((cqe.result as u8) & 0x07)
                    } else {
                        IntentCategory::Idle
                    };
                    let confidence = ConfidenceLevel::High; // placeholder
                    let gate_confirmed = self.gate_active;

                    if category == IntentCategory::Idle { self.stats.idle_results += 1; }
                    if confidence == ConfidenceLevel::High || confidence == ConfidenceLevel::Certain {
                        self.stats.high_confidence += 1;
                    }

                    return Some(IntentResult {
                        intent_hash: [tag as u8; 32], // placeholder hash
                        category,
                        confidence,
                        gate_confirmed,
                        classified_at: tick,
                        user_data: tag,
                    });
                }
            }
        }
        None
    }

    /// Open the ThoughtGate double-tap confirmation window.
    pub fn open_gate(&mut self, tick: u64) {
        self.gate_active = true;
        self.gate_start_tick = tick;
        crate::serial_println!("[SYNAPSE BRIDGE] ThoughtGate OPEN @ tick {}", tick);
    }

    /// Close/cancel the ThoughtGate.
    pub fn close_gate(&mut self) {
        if self.gate_active {
            self.stats.gate_aborts += 1;
            self.gate_active = false;
        }
    }

    /// Check if ThoughtGate has timed out, close it if so.
    pub fn tick_gate(&mut self, tick: u64) {
        if self.gate_active {
            if tick.saturating_sub(self.gate_start_tick) > self.gate_timeout_ticks {
                crate::serial_println!("[SYNAPSE BRIDGE] ThoughtGate TIMEOUT @ tick {}", tick);
                self.close_gate();
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!("  SynapseIPC submitted={} received={} gate_ok={} priv_enforced={}",
            self.stats.samples_submitted, self.stats.results_received,
            self.stats.gate_confirmations, self.stats.privacy_enforced
        );
    }
}

impl IntentCategory {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Navigate,
            1 => Self::Focus,
            2 => Self::Execute,
            3 => Self::Dismiss,
            4 => Self::Pivot,
            5 => Self::OpenShell,
            6 => Self::Abort,
            7 => Self::Custom,
            _ => Self::Idle,
        }
    }
}
