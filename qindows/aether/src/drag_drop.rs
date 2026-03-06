//! # Drag & Drop — Cross-Window + Cross-Silo DnD
//!
//! Handles drag-and-drop operations between windows and
//! across Silo boundaries with capability enforcement (Section 4.10).
//!
//! Features:
//! - Intra-window DnD (always allowed)
//! - Cross-window DnD within same Silo (allowed)
//! - Cross-Silo DnD (requires DragDrop capability)
//! - MIME type negotiation
//! - Visual feedback (drag cursor, drop targets)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Drag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    Idle,
    Dragging,
    OverTarget,
    Dropped,
    Cancelled,
}

/// Drop action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAction {
    Copy,
    Move,
    Link,
    None,
}

/// Drag data payload.
#[derive(Debug, Clone)]
pub struct DragPayload {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub label: String,
}

/// A drag session.
#[derive(Debug, Clone)]
pub struct DragSession {
    pub id: u64,
    pub source_silo: u64,
    pub source_window: u64,
    pub state: DragState,
    pub payloads: Vec<DragPayload>,
    pub allowed_actions: Vec<DropAction>,
    pub chosen_action: DropAction,
    pub target_silo: Option<u64>,
    pub target_window: Option<u64>,
    pub started_at: u64,
}

/// A drop target.
#[derive(Debug, Clone)]
pub struct DropTarget {
    pub window_id: u64,
    pub silo_id: u64,
    pub accepted_types: Vec<String>,
    pub active: bool,
}

/// DnD statistics.
#[derive(Debug, Clone, Default)]
pub struct DndStats {
    pub drags_started: u64,
    pub drops_completed: u64,
    pub drops_cancelled: u64,
    pub cross_silo_drops: u64,
    pub denied_drops: u64,
}

/// The Drag & Drop Manager.
pub struct DragDrop {
    pub sessions: BTreeMap<u64, DragSession>,
    pub targets: BTreeMap<u64, DropTarget>,
    next_id: u64,
    pub stats: DndStats,
}

impl DragDrop {
    pub fn new() -> Self {
        DragDrop {
            sessions: BTreeMap::new(),
            targets: BTreeMap::new(),
            next_id: 1,
            stats: DndStats::default(),
        }
    }

    /// Register a drop target.
    pub fn register_target(&mut self, window_id: u64, silo_id: u64, types: Vec<&str>) {
        self.targets.insert(window_id, DropTarget {
            window_id, silo_id,
            accepted_types: types.into_iter().map(String::from).collect(),
            active: true,
        });
    }

    /// Start a drag.
    pub fn start_drag(&mut self, source_silo: u64, source_window: u64, payloads: Vec<DragPayload>, actions: Vec<DropAction>, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.sessions.insert(id, DragSession {
            id, source_silo, source_window,
            state: DragState::Dragging, payloads,
            allowed_actions: actions, chosen_action: DropAction::None,
            target_silo: None, target_window: None,
            started_at: now,
        });

        self.stats.drags_started += 1;
        id
    }

    /// Drag enters a target window.
    pub fn enter_target(&mut self, session_id: u64, window_id: u64) -> bool {
        let target = match self.targets.get(&window_id) {
            Some(t) if t.active => t,
            _ => return false,
        };
        let target_silo = target.silo_id;
        let accepted = target.accepted_types.clone();

        if let Some(session) = self.sessions.get_mut(&session_id) {
            let compatible = session.payloads.iter()
                .any(|p| accepted.iter().any(|a| a == &p.mime_type || a == "*/*"));

            if compatible {
                session.state = DragState::OverTarget;
                session.target_silo = Some(target_silo);
                session.target_window = Some(window_id);
                return true;
            }
        }
        false
    }

    /// Execute drop.
    pub fn drop(&mut self, session_id: u64, has_cap: bool) -> Result<DropAction, &'static str> {
        let session = self.sessions.get_mut(&session_id).ok_or("Session not found")?;
        if session.state != DragState::OverTarget {
            return Err("Not over a valid target");
        }

        let cross_silo = session.target_silo.map(|t| t != session.source_silo).unwrap_or(false);
        if cross_silo && !has_cap {
            session.state = DragState::Cancelled;
            self.stats.denied_drops += 1;
            return Err("No DragDrop capability for cross-Silo");
        }

        let action = session.allowed_actions.first().copied().unwrap_or(DropAction::Copy);
        session.state = DragState::Dropped;
        session.chosen_action = action;

        self.stats.drops_completed += 1;
        if cross_silo { self.stats.cross_silo_drops += 1; }
        Ok(action)
    }

    /// Cancel a drag.
    pub fn cancel(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.state = DragState::Cancelled;
            self.stats.drops_cancelled += 1;
        }
    }
}
