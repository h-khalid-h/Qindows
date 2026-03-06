//! # Mesh Gossip — Protocol for State Dissemination
//!
//! Implements a SWIM-style gossip protocol for propagating
//! membership and state changes across mesh nodes (Section 11.18).
//!
//! Features:
//! - Membership protocol (join/leave/fail)
//! - Infection-style state propagation
//! - Suspicion mechanism (indirect probing)
//! - Lamport timestamp ordering
//! - Configurable fanout and interval

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Node state in membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Alive,
    Suspect,
    Dead,
    Left,
}

/// A gossip message.
#[derive(Debug, Clone)]
pub struct GossipMessage {
    pub from: [u8; 32],
    pub lamport: u64,
    pub updates: Vec<StateUpdate>,
}

/// A state update for a node.
#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub node_id: [u8; 32],
    pub state: NodeState,
    pub incarnation: u64,
    pub lamport: u64,
}

/// Membership entry.
#[derive(Debug, Clone)]
pub struct MemberEntry {
    pub node_id: [u8; 32],
    pub state: NodeState,
    pub incarnation: u64,
    pub last_seen: u64,
    pub suspicion_start: Option<u64>,
}

/// Gossip statistics.
#[derive(Debug, Clone, Default)]
pub struct GossipStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub state_updates: u64,
    pub nodes_joined: u64,
    pub nodes_failed: u64,
    pub suspicions: u64,
}

/// The Mesh Gossip Engine.
pub struct MeshGossip {
    pub self_id: [u8; 32],
    pub members: BTreeMap<[u8; 32], MemberEntry>,
    pub lamport: u64,
    pub incarnation: u64,
    pub fanout: usize,
    pub suspicion_timeout: u64,
    pub stats: GossipStats,
}

impl MeshGossip {
    pub fn new(self_id: [u8; 32]) -> Self {
        let mut members = BTreeMap::new();
        members.insert(self_id, MemberEntry {
            node_id: self_id, state: NodeState::Alive,
            incarnation: 1, last_seen: 0, suspicion_start: None,
        });
        MeshGossip {
            self_id, members, lamport: 0, incarnation: 1,
            fanout: 3, suspicion_timeout: 5000, stats: GossipStats::default(),
        }
    }

    /// Process a received gossip message.
    pub fn receive(&mut self, msg: GossipMessage, now: u64) {
        self.stats.messages_received += 1;
        self.lamport = self.lamport.max(msg.lamport) + 1;

        for update in msg.updates {
            self.apply_update(update, now);
        }
    }

    /// Apply a single state update.
    fn apply_update(&mut self, update: StateUpdate, now: u64) {
        if let Some(member) = self.members.get_mut(&update.node_id) {
            // Only apply if newer incarnation, or same incarnation with higher-priority state
            if update.incarnation < member.incarnation { return; }
            if update.incarnation == member.incarnation && !state_overrides(update.state, member.state) { return; }

            // If we're being suspected, refute it
            if update.node_id == self.self_id && update.state == NodeState::Suspect {
                self.incarnation += 1;
                member.incarnation = self.incarnation;
                member.state = NodeState::Alive;
                return;
            }

            member.state = update.state;
            member.incarnation = update.incarnation;
            member.last_seen = now;
            if update.state == NodeState::Suspect {
                member.suspicion_start = Some(now);
                self.stats.suspicions += 1;
            } else {
                member.suspicion_start = None;
            }
        } else {
            // New node joining
            self.members.insert(update.node_id, MemberEntry {
                node_id: update.node_id, state: update.state,
                incarnation: update.incarnation, last_seen: now,
                suspicion_start: None,
            });
            self.stats.nodes_joined += 1;
        }
        self.stats.state_updates += 1;
    }

    /// Generate a gossip message to send.
    pub fn generate_message(&mut self) -> GossipMessage {
        self.lamport += 1;
        self.stats.messages_sent += 1;

        let updates: Vec<StateUpdate> = self.members.values()
            .filter(|m| m.state != NodeState::Left)
            .map(|m| StateUpdate {
                node_id: m.node_id, state: m.state,
                incarnation: m.incarnation, lamport: self.lamport,
            })
            .collect();

        GossipMessage {
            from: self.self_id, lamport: self.lamport, updates,
        }
    }

    /// Check for timed-out suspects.
    pub fn check_suspects(&mut self, now: u64) {
        let dead: Vec<[u8; 32]> = self.members.values()
            .filter(|m| m.state == NodeState::Suspect)
            .filter(|m| m.suspicion_start.map(|s| now.saturating_sub(s) > self.suspicion_timeout).unwrap_or(false))
            .map(|m| m.node_id)
            .collect();
        for id in dead {
            if let Some(m) = self.members.get_mut(&id) {
                m.state = NodeState::Dead;
                self.stats.nodes_failed += 1;
            }
        }
    }

    /// Get alive member count.
    pub fn alive_count(&self) -> usize {
        self.members.values().filter(|m| m.state == NodeState::Alive).count()
    }
}

/// State priority: Dead > Suspect > Alive.
fn state_overrides(new: NodeState, old: NodeState) -> bool {
    let priority = |s: NodeState| -> u8 {
        match s { NodeState::Alive => 0, NodeState::Suspect => 1, NodeState::Dead => 2, NodeState::Left => 3 }
    };
    priority(new) > priority(old)
}
