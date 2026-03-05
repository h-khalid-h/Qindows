//! # Thought-Gate — BCI Mental Double-Tap Handshake
//!
//! Prevents accidental firing of BCI commands (Section 6.2).
//! The user must perform a "mental double-tap" — two distinct
//! intentional signals within a short window — to confirm actions.
//!
//! Security layers:
//! 1. **Pattern Recognition**: ML model identifies the double-tap signature
//! 2. **Confidence Threshold**: Both taps must exceed confidence minimum
//! 3. **Timing Window**: Taps must be 200-800ms apart (configurable)
//! 4. **Cooldown**: After activation, brief cooldown prevents spam
//! 5. **Intent Binding**: Gate binds to a specific pending intent

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Gate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    /// Idle — waiting for first tap
    Idle,
    /// First tap received — waiting for second
    FirstTap,
    /// Both taps received — gate open (action confirmed)
    Open,
    /// Cooldown after activation
    Cooldown,
    /// Failed (taps didn't match or timed out)
    Failed,
}

/// A detected neural tap event.
#[derive(Debug, Clone)]
pub struct TapEvent {
    /// Timestamp (microseconds)
    pub timestamp: u64,
    /// Confidence (0.0–1.0)
    pub confidence: f32,
    /// Neural pattern hash
    pub pattern_hash: u64,
    /// Signal amplitude
    pub amplitude: f32,
    /// Which electrode cluster detected the tap
    pub cluster_id: u8,
}

/// A pending intent awaiting gate confirmation.
#[derive(Debug, Clone)]
pub struct PendingIntent {
    /// Intent ID
    pub id: u64,
    /// Description (for UI display)
    pub description: String,
    /// Required confidence for this intent
    pub required_confidence: f32,
    /// Timestamp when intent was created
    pub created_at: u64,
    /// Timeout (microseconds from creation)
    pub timeout_us: u64,
    /// Associated Silo ID
    pub silo_id: u64,
}

/// Gate configuration.
#[derive(Debug, Clone)]
pub struct GateConfig {
    /// Minimum confidence for each tap (0.0–1.0)
    pub min_confidence: f32,
    /// Minimum gap between taps (microseconds)
    pub min_gap_us: u64,
    /// Maximum gap between taps (microseconds)
    pub max_gap_us: u64,
    /// Cooldown after successful activation (microseconds)
    pub cooldown_us: u64,
    /// Maximum pattern hash difference between taps
    pub max_pattern_drift: u64,
    /// Intent timeout (microseconds)
    pub intent_timeout_us: u64,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            min_confidence: 0.85,
            min_gap_us: 200_000,    // 200ms
            max_gap_us: 800_000,    // 800ms
            cooldown_us: 1_000_000, // 1 second
            max_pattern_drift: 100,
            intent_timeout_us: 10_000_000, // 10 seconds
        }
    }
}

/// Thought-Gate statistics.
#[derive(Debug, Clone, Default)]
pub struct GateStats {
    pub taps_received: u64,
    pub gates_opened: u64,
    pub gates_failed: u64,
    pub intents_confirmed: u64,
    pub intents_timed_out: u64,
    pub false_positives: u64,
    pub cooldowns_triggered: u64,
}

/// The Thought-Gate.
pub struct ThoughtGate {
    /// Current state
    pub state: GateState,
    /// Configuration
    pub config: GateConfig,
    /// First tap (if in FirstTap state)
    first_tap: Option<TapEvent>,
    /// Pending intents awaiting confirmation
    pub pending_intents: BTreeMap<u64, PendingIntent>,
    /// Confirmed intent ID (after gate opens)
    pub confirmed_intent: Option<u64>,
    /// Cooldown expires at (timestamp)
    cooldown_until: u64,
    /// Next intent ID
    next_intent_id: u64,
    /// Tap history (for ML model training)
    pub tap_history: Vec<TapEvent>,
    /// Max history entries
    pub max_history: usize,
    /// Statistics
    pub stats: GateStats,
}

impl ThoughtGate {
    pub fn new() -> Self {
        ThoughtGate {
            state: GateState::Idle,
            config: GateConfig::default(),
            first_tap: None,
            pending_intents: BTreeMap::new(),
            confirmed_intent: None,
            cooldown_until: 0,
            next_intent_id: 1,
            tap_history: Vec::new(),
            max_history: 1000,
            stats: GateStats::default(),
        }
    }

    /// Register a pending intent for gate confirmation.
    pub fn register_intent(
        &mut self,
        description: &str,
        silo_id: u64,
        required_confidence: f32,
        now: u64,
    ) -> u64 {
        let id = self.next_intent_id;
        self.next_intent_id += 1;

        self.pending_intents.insert(id, PendingIntent {
            id,
            description: String::from(description),
            required_confidence,
            created_at: now,
            timeout_us: self.config.intent_timeout_us,
            silo_id,
        });

        id
    }

    /// Process a neural tap event.
    pub fn on_tap(&mut self, tap: TapEvent) -> GateState {
        self.stats.taps_received += 1;

        // Record history
        if self.tap_history.len() >= self.max_history {
            self.tap_history.remove(0);
        }
        self.tap_history.push(tap.clone());

        // Check cooldown
        if self.state == GateState::Cooldown {
            if tap.timestamp < self.cooldown_until {
                return GateState::Cooldown;
            }
            self.state = GateState::Idle;
        }

        // Reject low-confidence taps
        if tap.confidence < self.config.min_confidence {
            return self.state;
        }

        match self.state {
            GateState::Idle => {
                // First tap — start the window
                self.first_tap = Some(tap);
                self.state = GateState::FirstTap;
                GateState::FirstTap
            }
            GateState::FirstTap => {
                if let Some(ref first) = self.first_tap {
                    let gap = tap.timestamp.saturating_sub(first.timestamp);

                    // Check timing window
                    if gap < self.config.min_gap_us {
                        // Too fast — likely noise, ignore
                        return GateState::FirstTap;
                    }
                    if gap > self.config.max_gap_us {
                        // Too slow — restart
                        self.first_tap = Some(tap);
                        self.stats.gates_failed += 1;
                        return GateState::FirstTap;
                    }

                    // Check pattern consistency
                    let drift = if tap.pattern_hash > first.pattern_hash {
                        tap.pattern_hash - first.pattern_hash
                    } else {
                        first.pattern_hash - tap.pattern_hash
                    };

                    if drift > self.config.max_pattern_drift {
                        // Pattern mismatch — not the same user or not intentional
                        self.state = GateState::Failed;
                        self.first_tap = None;
                        self.stats.gates_failed += 1;
                        return GateState::Failed;
                    }

                    // Gate opens!
                    self.state = GateState::Open;
                    self.stats.gates_opened += 1;

                    // Confirm the highest-priority pending intent
                    self.confirm_top_intent(tap.confidence);

                    // Enter cooldown
                    self.cooldown_until = tap.timestamp + self.config.cooldown_us;
                    self.state = GateState::Cooldown;
                    self.stats.cooldowns_triggered += 1;
                    self.first_tap = None;

                    GateState::Open
                } else {
                    self.state = GateState::Idle;
                    GateState::Idle
                }
            }
            _ => {
                // Reset to idle for unexpected states
                self.state = GateState::Idle;
                self.first_tap = None;
                GateState::Idle
            }
        }
    }

    /// Confirm the highest-priority pending intent.
    fn confirm_top_intent(&mut self, confidence: f32) {
        let top = self.pending_intents.values()
            .filter(|i| confidence >= i.required_confidence)
            .min_by_key(|i| i.created_at);

        if let Some(intent) = top {
            self.confirmed_intent = Some(intent.id);
            let id = intent.id;
            self.pending_intents.remove(&id);
            self.stats.intents_confirmed += 1;
        }
    }

    /// Expire timed-out intents.
    pub fn expire_intents(&mut self, now: u64) {
        let expired: Vec<u64> = self.pending_intents.iter()
            .filter(|(_, i)| now.saturating_sub(i.created_at) > i.timeout_us)
            .map(|(&id, _)| id)
            .collect();

        for id in expired {
            self.pending_intents.remove(&id);
            self.stats.intents_timed_out += 1;
        }
    }

    /// Take the confirmed intent (clears it).
    pub fn take_confirmed(&mut self) -> Option<u64> {
        self.confirmed_intent.take()
    }

    /// Check if the gate is ready to accept taps.
    pub fn is_ready(&self, now: u64) -> bool {
        match self.state {
            GateState::Idle | GateState::FirstTap => true,
            GateState::Cooldown => now >= self.cooldown_until,
            _ => false,
        }
    }
}
