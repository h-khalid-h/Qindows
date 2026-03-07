//! # Mesh Relay — NAT Traversal & Relay Nodes
//!
//! When direct peer-to-peer connections fail (symmetric NAT,
//! firewalls), traffic routes through relay nodes (Section 11.4).
//!
//! Features:
//! - STUN-like NAT type detection
//! - TURN-like relay for symmetric NAT
//! - Relay node selection based on latency + reputation
//! - Bandwidth throttling per relay session
//! - Automatic fallback: direct → hole-punch → relay

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;

/// NAT type detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    Open,
    FullCone,
    RestrictedCone,
    PortRestricted,
    Symmetric,
    Unknown,
}

/// Connection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Direct,
    HolePunch,
    Relay,
}

/// Relay session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Active,
    Throttled,
    Closed,
}

/// A relay node.
#[derive(Debug, Clone)]
pub struct RelayNode {
    pub id: [u8; 32],
    pub name: String,
    pub latency_ms: u32,
    pub bandwidth_mbps: u32,
    pub reputation: u8,
    pub active_sessions: u32,
    pub max_sessions: u32,
}

/// A relay session.
#[derive(Debug, Clone)]
pub struct RelaySession {
    pub id: u64,
    pub peer_a: [u8; 32],
    pub peer_b: [u8; 32],
    pub relay_id: [u8; 32],
    pub state: SessionState,
    pub strategy: Strategy,
    pub bytes_relayed: u64,
    pub bandwidth_limit: u64,
    pub created_at: u64,
}

/// Relay statistics.
#[derive(Debug, Clone, Default)]
pub struct RelayStats {
    pub direct_connections: u64,
    pub hole_punches: u64,
    pub relay_connections: u64,
    pub bytes_relayed: u64,
    pub sessions_created: u64,
    pub sessions_closed: u64,
}

/// The Mesh Relay Manager.
pub struct MeshRelay {
    pub relays: BTreeMap<[u8; 32], RelayNode>,
    pub sessions: BTreeMap<u64, RelaySession>,
    pub local_nat: NatType,
    next_session_id: u64,
    pub stats: RelayStats,
}

impl MeshRelay {
    pub fn new() -> Self {
        MeshRelay {
            relays: BTreeMap::new(),
            sessions: BTreeMap::new(),
            local_nat: NatType::Unknown,
            next_session_id: 1,
            stats: RelayStats::default(),
        }
    }

    /// Register a relay node.
    pub fn add_relay(&mut self, id: [u8; 32], name: &str, latency: u32, bw: u32, rep: u8) {
        self.relays.insert(id, RelayNode {
            id, name: String::from(name),
            latency_ms: latency, bandwidth_mbps: bw,
            reputation: rep, active_sessions: 0, max_sessions: 100,
        });
    }

    /// Detect NAT type (simplified).
    pub fn detect_nat(&mut self, external_port_matches: bool, restricted: bool) {
        self.local_nat = match (external_port_matches, restricted) {
            (true, false) => NatType::FullCone,
            (true, true) => NatType::RestrictedCone,
            (false, true) => NatType::Symmetric,
            (false, false) => NatType::PortRestricted,
        };
    }

    /// Choose connection strategy for a peer.
    pub fn choose_strategy(&self, peer_nat: NatType) -> Strategy {
        match (self.local_nat, peer_nat) {
            (NatType::Open, _) | (_, NatType::Open) => Strategy::Direct,
            (NatType::FullCone, _) | (_, NatType::FullCone) => Strategy::Direct,
            (NatType::Symmetric, NatType::Symmetric) => Strategy::Relay,
            _ => Strategy::HolePunch,
        }
    }

    /// Create a relay session.
    pub fn connect(&mut self, peer_a: [u8; 32], peer_b: [u8; 32], peer_b_nat: NatType, now: u64) -> Result<u64, &'static str> {
        let strategy = self.choose_strategy(peer_b_nat);

        match strategy {
            Strategy::Direct => {
                self.stats.direct_connections += 1;
            }
            Strategy::HolePunch => {
                self.stats.hole_punches += 1;
            }
            Strategy::Relay => {
                self.stats.relay_connections += 1;
            }
        }

        // Select best relay (lowest latency with capacity)
        let relay_id = if strategy == Strategy::Relay {
            let best = self.relays.values()
                .filter(|r| r.active_sessions < r.max_sessions)
                .min_by_key(|r| r.latency_ms)
                .ok_or("No relay available")?;
            best.id
        } else {
            [0u8; 32] // No relay needed
        };

        let id = self.next_session_id;
        self.next_session_id += 1;

        if strategy == Strategy::Relay {
            if let Some(relay) = self.relays.get_mut(&relay_id) {
                relay.active_sessions += 1;
            }
        }

        self.sessions.insert(id, RelaySession {
            id, peer_a, peer_b, relay_id,
            state: SessionState::Active,
            strategy, bytes_relayed: 0,
            bandwidth_limit: 10_000_000, // 10 MB/s default
            created_at: now,
        });

        self.stats.sessions_created += 1;
        Ok(id)
    }

    /// Close a session.
    pub fn close(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.state = SessionState::Closed;
            self.stats.bytes_relayed += session.bytes_relayed;

            if session.strategy == Strategy::Relay {
                if let Some(relay) = self.relays.get_mut(&session.relay_id) {
                    relay.active_sessions = relay.active_sessions.saturating_sub(1);
                }
            }
            self.stats.sessions_closed += 1;
        }
    }
}
