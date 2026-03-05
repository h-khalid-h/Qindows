//! # Voice Command Engine — Wake-Word + Command Parsing
//!
//! Processes audio input to detect wake-words and parse
//! voice commands into structured intents (Section 6.3).
//!
//! Pipeline:
//! 1. **Wake-word detection**: "Hey Qindows" activates listening
//! 2. **Speech-to-text**: Neural ASR produces transcript
//! 3. **Intent parsing**: NLU extracts command + parameters
//! 4. **Confirmation**: High-risk commands require Thought-Gate

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Voice engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Confirming,
}

/// A parsed voice intent.
#[derive(Debug, Clone)]
pub struct VoiceIntent {
    pub id: u64,
    pub command: String,
    pub parameters: BTreeMap<String, String>,
    pub confidence: f32,
    pub transcript: String,
    pub requires_confirm: bool,
    pub timestamp: u64,
}

/// A registered voice command pattern.
#[derive(Debug, Clone)]
pub struct CommandPattern {
    pub name: String,
    pub keywords: Vec<String>,
    pub requires_confirm: bool,
    pub silo_id: Option<u64>,
}

/// Voice engine statistics.
#[derive(Debug, Clone, Default)]
pub struct VoiceStats {
    pub wake_detections: u64,
    pub commands_parsed: u64,
    pub commands_executed: u64,
    pub commands_rejected: u64,
    pub false_wakes: u64,
}

/// The Voice Command Engine.
pub struct VoiceEngine {
    pub state: VoiceState,
    pub commands: Vec<CommandPattern>,
    pub pending_intent: Option<VoiceIntent>,
    pub wake_word: String,
    pub min_confidence: f32,
    next_intent_id: u64,
    /// Listening timeout (ms)
    pub listen_timeout_ms: u64,
    pub listen_start: u64,
    pub stats: VoiceStats,
}

impl VoiceEngine {
    pub fn new() -> Self {
        VoiceEngine {
            state: VoiceState::Idle,
            commands: Vec::new(),
            pending_intent: None,
            wake_word: String::from("hey qindows"),
            min_confidence: 0.7,
            next_intent_id: 1,
            listen_timeout_ms: 5000,
            listen_start: 0,
            stats: VoiceStats::default(),
        }
    }

    /// Register a voice command.
    pub fn register_command(&mut self, name: &str, keywords: Vec<&str>, requires_confirm: bool, silo_id: Option<u64>) {
        self.commands.push(CommandPattern {
            name: String::from(name),
            keywords: keywords.into_iter().map(String::from).collect(),
            requires_confirm,
            silo_id,
        });
    }

    /// Process wake-word detection.
    pub fn on_wake(&mut self, confidence: f32, now: u64) -> bool {
        if confidence < self.min_confidence {
            self.stats.false_wakes += 1;
            return false;
        }

        self.state = VoiceState::Listening;
        self.listen_start = now;
        self.stats.wake_detections += 1;
        true
    }

    /// Process speech-to-text result.
    pub fn on_transcript(&mut self, transcript: &str, confidence: f32, now: u64) -> Option<&VoiceIntent> {
        if self.state != VoiceState::Listening {
            return None;
        }

        self.state = VoiceState::Processing;

        // Match against registered commands
        let lower = transcript.to_ascii_lowercase();
        let mut best_match: Option<&CommandPattern> = None;
        let mut best_score = 0usize;

        for cmd in &self.commands {
            let score: usize = cmd.keywords.iter()
                .filter(|kw| lower.contains(kw.as_str()))
                .count();
            if score > best_score {
                best_score = score;
                best_match = Some(cmd);
            }
        }

        if let Some(cmd) = best_match {
            let id = self.next_intent_id;
            self.next_intent_id += 1;

            let intent = VoiceIntent {
                id,
                command: cmd.name.clone(),
                parameters: BTreeMap::new(),
                confidence,
                transcript: String::from(transcript),
                requires_confirm: cmd.requires_confirm,
                timestamp: now,
            };

            self.stats.commands_parsed += 1;

            if cmd.requires_confirm {
                self.state = VoiceState::Confirming;
            } else {
                self.state = VoiceState::Idle;
                self.stats.commands_executed += 1;
            }

            self.pending_intent = Some(intent);
            return self.pending_intent.as_ref();
        }

        self.state = VoiceState::Idle;
        self.stats.commands_rejected += 1;
        None
    }

    /// Confirm a pending intent (via Thought-Gate or explicit).
    pub fn confirm(&mut self) -> Option<VoiceIntent> {
        if self.state != VoiceState::Confirming {
            return None;
        }
        self.state = VoiceState::Idle;
        self.stats.commands_executed += 1;
        self.pending_intent.take()
    }

    /// Reject a pending intent.
    pub fn reject(&mut self) {
        if self.state == VoiceState::Confirming {
            self.state = VoiceState::Idle;
            self.pending_intent = None;
            self.stats.commands_rejected += 1;
        }
    }

    /// Check for listening timeout.
    pub fn check_timeout(&mut self, now: u64) {
        if self.state == VoiceState::Listening {
            if now.saturating_sub(self.listen_start) > self.listen_timeout_ms {
                self.state = VoiceState::Idle;
            }
        }
    }
}
