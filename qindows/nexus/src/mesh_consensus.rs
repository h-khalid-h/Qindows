//! # Mesh Consensus — Raft-Based Distributed Agreement
//!
//! Implements a simplified Raft consensus protocol for
//! distributed state agreement across mesh nodes (Section 11.11).
//!
//! Features:
//! - Leader election with term tracking
//! - Log replication
//! - Commit index advancement
//! - Node state machine (Follower/Candidate/Leader)
//! - Heartbeat-based failure detection

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Raft node role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

/// A log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub index: u64,
    pub term: u64,
    pub data: Vec<u8>,
}

/// Raft node state.
#[derive(Debug, Clone)]
pub struct RaftNode {
    pub node_id: [u8; 32],
    pub next_index: u64,
    pub match_index: u64,
    pub voted_for_us: bool,
    pub last_heartbeat: u64,
}

/// Consensus statistics.
#[derive(Debug, Clone, Default)]
pub struct ConsensusStats {
    pub elections_started: u64,
    pub elections_won: u64,
    pub entries_committed: u64,
    pub entries_replicated: u64,
    pub heartbeats_sent: u64,
}

/// The Raft Consensus Engine.
pub struct MeshConsensus {
    pub self_id: [u8; 32],
    pub role: RaftRole,
    pub current_term: u64,
    pub voted_for: Option<[u8; 32]>,
    pub log: Vec<LogEntry>,
    pub commit_index: u64,
    pub last_applied: u64,
    pub nodes: BTreeMap<[u8; 32], RaftNode>,
    pub election_timeout_ms: u64,
    pub heartbeat_interval_ms: u64,
    pub stats: ConsensusStats,
}

impl MeshConsensus {
    pub fn new(self_id: [u8; 32]) -> Self {
        MeshConsensus {
            self_id, role: RaftRole::Follower,
            current_term: 0, voted_for: None,
            log: Vec::new(), commit_index: 0, last_applied: 0,
            nodes: BTreeMap::new(),
            election_timeout_ms: 300,
            heartbeat_interval_ms: 100,
            stats: ConsensusStats::default(),
        }
    }

    /// Add a peer node.
    pub fn add_peer(&mut self, node_id: [u8; 32]) {
        self.nodes.insert(node_id, RaftNode {
            node_id, next_index: 1, match_index: 0,
            voted_for_us: false, last_heartbeat: 0,
        });
    }

    /// Start an election (become Candidate).
    pub fn start_election(&mut self) {
        self.current_term += 1;
        self.role = RaftRole::Candidate;
        self.voted_for = Some(self.self_id);
        self.stats.elections_started += 1;

        // Reset votes
        for node in self.nodes.values_mut() {
            node.voted_for_us = false;
        }
    }

    /// Receive a vote from a peer.
    pub fn receive_vote(&mut self, from: [u8; 32], term: u64, granted: bool) {
        if term != self.current_term || self.role != RaftRole::Candidate {
            return;
        }
        if let Some(node) = self.nodes.get_mut(&from) {
            node.voted_for_us = granted;
        }

        // Check if we have a majority
        let votes = self.nodes.values().filter(|n| n.voted_for_us).count() + 1; // +1 for self
        let total = self.nodes.len() + 1;
        if votes > total / 2 {
            self.role = RaftRole::Leader;
            self.stats.elections_won += 1;
            // Initialize next_index for all peers
            let last_log = self.log.len() as u64 + 1;
            for node in self.nodes.values_mut() {
                node.next_index = last_log;
                node.match_index = 0;
            }
        }
    }

    /// Append an entry (Leader only).
    pub fn append(&mut self, data: Vec<u8>) -> Option<u64> {
        if self.role != RaftRole::Leader { return None; }
        let index = self.log.len() as u64 + 1;
        self.log.push(LogEntry {
            index, term: self.current_term, data,
        });
        Some(index)
    }

    /// Acknowledge replication from a peer.
    pub fn ack_replication(&mut self, from: [u8; 32], match_index: u64) {
        if let Some(node) = self.nodes.get_mut(&from) {
            node.match_index = match_index;
            node.next_index = match_index + 1;
        }
        self.stats.entries_replicated += 1;
        self.advance_commit();
    }

    /// Advance commit index based on replication quorum.
    fn advance_commit(&mut self) {
        let total = self.nodes.len() + 1;
        let majority = total / 2 + 1;

        for idx in (self.commit_index + 1)..=(self.log.len() as u64) {
            let replicated = self.nodes.values()
                .filter(|n| n.match_index >= idx)
                .count() + 1; // +1 for leader

            if replicated >= majority {
                self.commit_index = idx;
                self.stats.entries_committed += 1;
            } else {
                break;
            }
        }
    }

    /// Step down to Follower (e.g., on higher term).
    pub fn step_down(&mut self, new_term: u64) {
        if new_term > self.current_term {
            self.current_term = new_term;
            self.role = RaftRole::Follower;
            self.voted_for = None;
        }
    }
}
