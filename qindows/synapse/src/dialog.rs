//! # Synapse Dialog Manager
//!
//! Multi-turn dialog state tracking for the Synapse AI assistant.
//! Maintains conversation context, slot filling, disambiguation,
//! and follow-up question generation.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Dialog state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogState {
    /// Waiting for user input
    Idle,
    /// Actively processing a multi-turn task
    Active,
    /// Waiting for slot confirmation
    ConfirmSlot,
    /// Disambiguating between options
    Disambiguating,
    /// Task completed
    Completed,
    /// Error state
    Error,
}

/// A dialog turn.
#[derive(Debug, Clone)]
pub struct Turn {
    /// Turn number (1-based)
    pub number: u32,
    /// Who spoke: true = user, false = assistant
    pub is_user: bool,
    /// The utterance
    pub text: String,
    /// Intent detected (if user turn)
    pub intent: Option<String>,
    /// Slots extracted
    pub slots: BTreeMap<String, String>,
    /// Timestamp (ns)
    pub timestamp: u64,
}

/// A required slot for task completion.
#[derive(Debug, Clone)]
pub struct RequiredSlot {
    /// Slot name
    pub name: String,
    /// Human-readable prompt to fill this slot
    pub prompt: String,
    /// Has been filled?
    pub filled: bool,
    /// Current value
    pub value: Option<String>,
    /// Validation function tag (for type checking)
    pub validator: SlotValidator,
}

/// Slot value validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotValidator {
    /// Any string
    AnyString,
    /// Must be a number
    Number,
    /// Must be a file path
    FilePath,
    /// Must be a time expression
    Time,
    /// Must be yes/no
    Boolean,
    /// Must be from a specific list (stored externally)
    Enum,
}

/// A dialog session.
#[derive(Debug, Clone)]
pub struct DialogSession {
    /// Session ID
    pub id: u64,
    /// Silo ID
    pub silo_id: u64,
    /// Dialog state
    pub state: DialogState,
    /// Conversation history
    pub turns: Vec<Turn>,
    /// Active intent
    pub active_intent: Option<String>,
    /// Slots being filled
    pub slots: Vec<RequiredSlot>,
    /// Context variables (carry-over data)
    pub context: BTreeMap<String, String>,
    /// Disambiguation options
    pub options: Vec<String>,
    /// Started at
    pub started_at: u64,
    /// Max turns before timeout
    pub max_turns: u32,
}

impl DialogSession {
    pub fn new(id: u64, silo_id: u64, now: u64) -> Self {
        DialogSession {
            id, silo_id,
            state: DialogState::Idle,
            turns: Vec::new(),
            active_intent: None,
            slots: Vec::new(),
            context: BTreeMap::new(),
            options: Vec::new(),
            started_at: now,
            max_turns: 20,
        }
    }

    /// Add a user turn.
    pub fn user_says(&mut self, text: &str, now: u64) -> u32 {
        let turn_num = self.turns.len() as u32 + 1;
        self.turns.push(Turn {
            number: turn_num,
            is_user: true,
            text: String::from(text),
            intent: None,
            slots: BTreeMap::new(),
            timestamp: now,
        });
        self.state = DialogState::Active;
        turn_num
    }

    /// Add an assistant response.
    pub fn assistant_says(&mut self, text: &str, now: u64) {
        let turn_num = self.turns.len() as u32 + 1;
        self.turns.push(Turn {
            number: turn_num,
            is_user: false,
            text: String::from(text),
            intent: None,
            slots: BTreeMap::new(),
            timestamp: now,
        });
    }

    /// Set the active intent and required slots.
    pub fn set_intent(&mut self, intent: &str, required_slots: Vec<RequiredSlot>) {
        self.active_intent = Some(String::from(intent));
        self.slots = required_slots;
        self.state = DialogState::Active;
    }

    /// Fill a slot value.
    pub fn fill_slot(&mut self, name: &str, value: &str) -> bool {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.name == name) {
            // Validate
            let valid = match slot.validator {
                SlotValidator::AnyString => true,
                SlotValidator::Number => value.parse::<f64>().is_ok(),
                SlotValidator::FilePath => !value.is_empty() && value.contains('/'),
                SlotValidator::Time => value.contains(':') || value.contains("am") || value.contains("pm"),
                SlotValidator::Boolean => {
                    let lower = value.to_lowercase();
                    lower == "yes" || lower == "no" || lower == "true" || lower == "false"
                }
                SlotValidator::Enum => true, // External validation
            };

            if valid {
                slot.value = Some(String::from(value));
                slot.filled = true;
                return true;
            }
        }
        false
    }

    /// Get the next unfilled required slot.
    pub fn next_unfilled_slot(&self) -> Option<&RequiredSlot> {
        self.slots.iter().find(|s| !s.filled)
    }

    /// Are all required slots filled?
    pub fn all_slots_filled(&self) -> bool {
        self.slots.iter().all(|s| s.filled)
    }

    /// Enter disambiguation state.
    pub fn disambiguate(&mut self, options: Vec<String>) {
        self.options = options;
        self.state = DialogState::Disambiguating;
    }

    /// Resolve disambiguation by index.
    pub fn resolve_disambiguation(&mut self, index: usize) -> Option<String> {
        if index < self.options.len() {
            let choice = self.options[index].clone();
            self.options.clear();
            self.state = DialogState::Active;
            Some(choice)
        } else {
            None
        }
    }

    /// Complete the dialog.
    pub fn complete(&mut self) {
        self.state = DialogState::Completed;
    }

    /// Is the dialog expired?
    pub fn is_expired(&self) -> bool {
        self.turns.len() as u32 >= self.max_turns
    }

    /// Get all filled slot values.
    pub fn filled_values(&self) -> BTreeMap<String, String> {
        self.slots.iter()
            .filter_map(|s| s.value.as_ref().map(|v| (s.name.clone(), v.clone())))
            .collect()
    }

    /// Set a context variable.
    pub fn set_context(&mut self, key: &str, value: &str) {
        self.context.insert(String::from(key), String::from(value));
    }
}

/// The Dialog Manager.
pub struct DialogManager {
    /// Active sessions
    pub sessions: BTreeMap<u64, DialogSession>,
    /// Next session ID
    next_id: u64,
    /// Stats
    pub total_sessions: u64,
    pub total_turns: u64,
    pub completed_sessions: u64,
}

impl DialogManager {
    pub fn new() -> Self {
        DialogManager {
            sessions: BTreeMap::new(),
            next_id: 1,
            total_sessions: 0,
            total_turns: 0,
            completed_sessions: 0,
        }
    }

    /// Start a new dialog session.
    pub fn start_session(&mut self, silo_id: u64, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(id, DialogSession::new(id, silo_id, now));
        self.total_sessions += 1;
        id
    }

    /// Process user input in a session.
    pub fn process_input(&mut self, session_id: u64, text: &str, now: u64) -> Option<String> {
        let session = self.sessions.get_mut(&session_id)?;
        session.user_says(text, now);
        self.total_turns += 1;

        // Check if we're disambiguating
        if session.state == DialogState::Disambiguating {
            if let Ok(idx) = text.parse::<usize>() {
                if let Some(choice) = session.resolve_disambiguation(idx.saturating_sub(1)) {
                    return Some(alloc::format!("Selected: {}", choice));
                }
            }
            return Some(String::from("Please choose a number from the options."));
        }

        // Check if we need to fill slots
        if let Some(slot) = session.next_unfilled_slot() {
            let prompt = slot.prompt.clone();
            let name = slot.name.clone();
            session.fill_slot(&name, text);
            if session.all_slots_filled() {
                session.complete();
                self.completed_sessions += 1;
                return Some(String::from("All information collected. Processing..."));
            }
            if let Some(next) = session.next_unfilled_slot() {
                return Some(next.prompt.clone());
            }
            return Some(prompt);
        }

        None
    }

    /// End a session.
    pub fn end_session(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.complete();
            self.completed_sessions += 1;
        }
    }
}
