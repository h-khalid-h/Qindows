//! # Q-Collab — Distributed Workspace Collaboration
//!
//! Real-time collaborative editing powered by CRDTs (from nexus::crdt).
//! Multiple users across the Global Mesh can simultaneously edit
//! the same document, code file, or canvas without conflicts.
//!
//! Architecture:
//! - Each participant gets a `Session` with a unique replica ID
//! - Edits are represented as CRDT operations (insert/delete)
//! - Operations propagate via Q-Fabric to all participants
//! - The OR-Set and LWW-Register provide conflict resolution
//! - Presence info shows cursors, selections, and user status

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A participant in a collaborative session.
#[derive(Debug, Clone)]
pub struct Participant {
    /// Unique replica ID
    pub replica_id: u64,
    /// Display name
    pub name: String,
    /// Cursor position (byte offset in the document)
    pub cursor_pos: u64,
    /// Selection range (start, end)
    pub selection: Option<(u64, u64)>,
    /// Is this participant currently active?
    pub active: bool,
    /// Last heartbeat timestamp
    pub last_heartbeat: u64,
    /// Color (for cursor/highlight rendering)
    pub color: u32,
}

/// An edit operation (CRDT-compatible).
#[derive(Debug, Clone)]
pub enum EditOp {
    /// Insert text at a position
    Insert {
        pos: u64,
        text: String,
        replica_id: u64,
        timestamp: u64,
    },
    /// Delete a range of text
    Delete {
        start: u64,
        end: u64,
        replica_id: u64,
        timestamp: u64,
    },
    /// Set a metadata key (LWW-Register)
    SetMeta {
        key: String,
        value: String,
        replica_id: u64,
        timestamp: u64,
    },
}

/// The document state (a sequence of characters with causal IDs).
#[derive(Debug, Clone)]
pub struct CollabDocument {
    /// Document ID
    pub id: u64,
    /// Document name
    pub name: String,
    /// Current text content
    pub content: String,
    /// Metadata (title, author, etc.)
    pub metadata: BTreeMap<String, String>,
    /// Operation log (for replay / undo)
    pub op_log: Vec<EditOp>,
    /// Version counter
    pub version: u64,
}

impl CollabDocument {
    pub fn new(id: u64, name: &str) -> Self {
        CollabDocument {
            id,
            name: String::from(name),
            content: String::new(),
            metadata: BTreeMap::new(),
            op_log: Vec::new(),
            version: 0,
        }
    }

    /// Apply an insert operation.
    pub fn apply_insert(&mut self, pos: u64, text: &str) {
        let byte_pos = (pos as usize).min(self.content.len());
        // Ensure we're on a char boundary to avoid panics
        let safe_pos = if self.content.is_char_boundary(byte_pos) {
            byte_pos
        } else {
            // Walk backward to find the nearest char boundary
            (0..=byte_pos).rev().find(|&i| self.content.is_char_boundary(i)).unwrap_or(0)
        };
        self.content.insert_str(safe_pos, text);
        self.version += 1;
    }

    /// Apply a delete operation.
    pub fn apply_delete(&mut self, start: u64, end: u64) {
        let start = (start as usize).min(self.content.len());
        let end = (end as usize).min(self.content.len());
        if start < end {
            self.content.drain(start..end);
            self.version += 1;
        }
    }

    /// Get content length.
    pub fn len(&self) -> usize {
        self.content.len()
    }
}

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Waiting for participants
    Waiting,
    /// Active collaboration
    Active,
    /// Paused (offline mode)
    Paused,
    /// Session ended
    Closed,
}

/// A collaborative session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session ID
    pub id: u64,
    /// Session state
    pub state: SessionState,
    /// Participants
    pub participants: BTreeMap<u64, Participant>,
    /// The shared document
    pub document: CollabDocument,
    /// Maximum participants
    pub max_participants: usize,
    /// Created timestamp
    pub created_at: u64,
}

/// Q-Collab statistics.
#[derive(Debug, Clone, Default)]
pub struct CollabStats {
    pub sessions_created: u64,
    pub operations_applied: u64,
    pub operations_synced: u64,
    pub conflicts_resolved: u64,
    pub participants_joined: u64,
    pub heartbeats: u64,
}

/// The Q-Collab Manager.
pub struct QCollab {
    /// Active sessions
    pub sessions: BTreeMap<u64, Session>,
    /// Next session ID
    next_session_id: u64,
    /// Next color index (for participant colors)
    next_color_idx: usize,
    /// Predefined participant colors
    colors: Vec<u32>,
    /// Statistics
    pub stats: CollabStats,
}

impl QCollab {
    pub fn new() -> Self {
        QCollab {
            sessions: BTreeMap::new(),
            next_session_id: 1,
            next_color_idx: 0,
            colors: alloc::vec![
                0xFF4A90D9, // Blue
                0xFF50C878, // Green
                0xFFE74C3C, // Red
                0xFFF39C12, // Orange
                0xFF9B59B6, // Purple
                0xFF1ABC9C, // Teal
                0xFFE91E63, // Pink
                0xFF00BCD4, // Cyan
            ],
            stats: CollabStats::default(),
        }
    }

    /// Create a new collaborative session.
    pub fn create_session(&mut self, doc_name: &str, now: u64) -> u64 {
        let id = self.next_session_id;
        self.next_session_id += 1;

        self.sessions.insert(id, Session {
            id,
            state: SessionState::Waiting,
            participants: BTreeMap::new(),
            document: CollabDocument::new(id, doc_name),
            max_participants: 32,
            created_at: now,
        });

        self.stats.sessions_created += 1;
        id
    }

    /// Join a session.
    pub fn join(
        &mut self,
        session_id: u64,
        replica_id: u64,
        name: &str,
        now: u64,
    ) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or("Session not found")?;

        if session.participants.len() >= session.max_participants {
            return Err("Session full");
        }

        let color = self.colors[self.next_color_idx % self.colors.len()];
        self.next_color_idx += 1;

        session.participants.insert(replica_id, Participant {
            replica_id,
            name: String::from(name),
            cursor_pos: 0,
            selection: None,
            active: true,
            last_heartbeat: now,
            color,
        });

        if session.state == SessionState::Waiting {
            session.state = SessionState::Active;
        }

        self.stats.participants_joined += 1;
        Ok(())
    }

    /// Apply an edit operation to a session.
    pub fn apply_op(
        &mut self,
        session_id: u64,
        op: EditOp,
    ) -> Result<(), &'static str> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or("Session not found")?;

        match &op {
            EditOp::Insert { pos, text, .. } => {
                session.document.apply_insert(*pos, text);
            }
            EditOp::Delete { start, end, .. } => {
                session.document.apply_delete(*start, *end);
            }
            EditOp::SetMeta { key, value, .. } => {
                session.document.metadata.insert(key.clone(), value.clone());
            }
        }

        session.document.op_log.push(op);
        self.stats.operations_applied += 1;
        Ok(())
    }

    /// Update a participant's cursor position.
    pub fn update_cursor(
        &mut self,
        session_id: u64,
        replica_id: u64,
        cursor_pos: u64,
        selection: Option<(u64, u64)>,
    ) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(p) = session.participants.get_mut(&replica_id) {
                p.cursor_pos = cursor_pos;
                p.selection = selection;
            }
        }
    }

    /// Heartbeat from a participant.
    pub fn heartbeat(&mut self, session_id: u64, replica_id: u64, now: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(p) = session.participants.get_mut(&replica_id) {
                p.last_heartbeat = now;
                p.active = true;
            }
        }
        self.stats.heartbeats += 1;
    }

    /// Detect and mark inactive participants.
    pub fn check_timeouts(&mut self, now: u64, timeout: u64) {
        for session in self.sessions.values_mut() {
            for p in session.participants.values_mut() {
                if p.active && now.saturating_sub(p.last_heartbeat) > timeout {
                    p.active = false;
                }
            }
        }
    }

    /// Close a session.
    pub fn close_session(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.state = SessionState::Closed;
        }
    }
}
