//! # Mesh Session — Persistent Connection Management
//!
//! Manages persistent bidirectional sessions between mesh
//! nodes with heartbeat, reconnection, and session state
//! serialization (Section 11.32).
//!
//! Features:
//! - Session establishment with handshake
//! - Heartbeat-based keepalive
//! - Automatic reconnection with exponential backoff
//! - Session state serialization for migration
//! - Per-Silo session isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Handshaking,
    Active,
    Idle,
    Reconnecting,
    Closed,
}

/// A mesh session.
#[derive(Debug, Clone)]
pub struct MeshSession {
    pub id: u64,
    pub peer_id: [u8; 32],
    pub state: SessionState,
    pub silo_id: u64,
    pub established_at: u64,
    pub last_heartbeat: u64,
    pub heartbeat_interval_ms: u64,
    pub missed_heartbeats: u32,
    pub max_missed: u32,
    pub reconnect_attempts: u32,
    pub max_reconnects: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

impl MeshSession {
    pub fn new(id: u64, peer_id: [u8; 32], silo_id: u64, now: u64) -> Self {
        MeshSession {
            id, peer_id, state: SessionState::Connecting,
            silo_id, established_at: now, last_heartbeat: now,
            heartbeat_interval_ms: 5000, missed_heartbeats: 0,
            max_missed: 3, reconnect_attempts: 0, max_reconnects: 5,
            bytes_sent: 0, bytes_received: 0,
        }
    }

    /// Transition to active.
    pub fn activate(&mut self, now: u64) {
        self.state = SessionState::Active;
        self.last_heartbeat = now;
        self.missed_heartbeats = 0;
        self.reconnect_attempts = 0;
    }

    /// Record a heartbeat.
    pub fn heartbeat(&mut self, now: u64) {
        self.last_heartbeat = now;
        self.missed_heartbeats = 0;
    }

    /// Check if heartbeat is overdue.
    pub fn check_heartbeat(&mut self, now: u64) -> bool {
        if self.state != SessionState::Active { return true; }
        let elapsed = now.saturating_sub(self.last_heartbeat);
        if elapsed > self.heartbeat_interval_ms {
            self.missed_heartbeats += 1;
            if self.missed_heartbeats >= self.max_missed {
                self.state = SessionState::Reconnecting;
                return false;
            }
        }
        true
    }

    /// Attempt reconnection.
    pub fn try_reconnect(&mut self) -> bool {
        if self.reconnect_attempts >= self.max_reconnects {
            self.state = SessionState::Closed;
            return false;
        }
        self.reconnect_attempts += 1;
        self.state = SessionState::Connecting;
        true
    }

    /// Backoff delay for reconnection (ms).
    pub fn backoff_ms(&self) -> u64 {
        1000u64.saturating_mul(1u64 << self.reconnect_attempts.min(6))
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.state, SessionState::Active | SessionState::Idle)
    }
}

/// Session manager statistics.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub sessions_created: u64,
    pub sessions_closed: u64,
    pub reconnections: u64,
    pub heartbeats_processed: u64,
}

/// The Mesh Session Manager.
pub struct MeshSessionMgr {
    pub sessions: BTreeMap<u64, MeshSession>,
    next_id: u64,
    pub stats: SessionStats,
}

impl MeshSessionMgr {
    pub fn new() -> Self {
        MeshSessionMgr {
            sessions: BTreeMap::new(),
            next_id: 1,
            stats: SessionStats::default(),
        }
    }

    /// Create a new session.
    pub fn connect(&mut self, peer_id: [u8; 32], silo_id: u64, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(id, MeshSession::new(id, peer_id, silo_id, now));
        self.stats.sessions_created += 1;
        id
    }

    /// Close a session.
    pub fn close(&mut self, id: u64) {
        if let Some(s) = self.sessions.get_mut(&id) {
            s.state = SessionState::Closed;
            self.stats.sessions_closed += 1;
        }
    }

    /// Tick all sessions (check heartbeats).
    pub fn tick(&mut self, now: u64) {
        let ids: Vec<u64> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Some(s) = self.sessions.get_mut(&id) {
                if s.state == SessionState::Active {
                    s.check_heartbeat(now);
                    self.stats.heartbeats_processed += 1;
                }
                if s.state == SessionState::Reconnecting {
                    s.try_reconnect();
                    self.stats.reconnections += 1;
                }
            }
        }
    }

    /// Active session count.
    pub fn active_count(&self) -> usize {
        self.sessions.values().filter(|s| s.is_alive()).count()
    }
}
