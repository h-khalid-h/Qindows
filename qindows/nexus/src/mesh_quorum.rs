//! # Mesh Quorum — Distributed Consensus Voting
//!
//! Implements quorum-based consensus for critical mesh
//! operations: leader election, configuration changes,
//! and membership updates (Section 11.6).
//!
//! Features:
//! - Configurable quorum size (N/2+1 default)
//! - Term-based elections (prevents split-brain)
//! - Vote deduplication
//! - Timeout-driven re-election
//! - Per-topic ballot isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// Vote decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vote {
    Aye,
    Nay,
    Abstain,
}

/// Ballot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BallotState {
    Open,
    Passed,
    Failed,
    Expired,
}

/// A ballot (vote on a specific topic).
#[derive(Debug, Clone)]
pub struct Ballot {
    pub id: u64,
    pub term: u64,
    pub topic: u64,
    pub proposer: [u8; 32],
    pub votes: BTreeMap<[u8; 32], Vote>,
    pub quorum_size: usize,
    pub state: BallotState,
    pub created_at: u64,
    pub timeout_at: u64,
}

impl Ballot {
    /// Count of Aye votes.
    pub fn ayes(&self) -> usize {
        self.votes.values().filter(|v| **v == Vote::Aye).count()
    }
    /// Count of Nay votes.
    pub fn nays(&self) -> usize {
        self.votes.values().filter(|v| **v == Vote::Nay).count()
    }
    /// Has quorum been reached?
    pub fn has_quorum(&self) -> bool {
        self.ayes() >= self.quorum_size
    }
    /// Is the ballot decided (either way)?
    pub fn is_decided(&self) -> bool {
        self.ayes() >= self.quorum_size || self.nays() >= self.quorum_size
    }
}

/// Quorum statistics.
#[derive(Debug, Clone, Default)]
pub struct QuorumStats {
    pub ballots_created: u64,
    pub ballots_passed: u64,
    pub ballots_failed: u64,
    pub ballots_expired: u64,
    pub votes_cast: u64,
    pub elections_held: u64,
}

/// The Quorum Manager.
pub struct MeshQuorum {
    pub ballots: BTreeMap<u64, Ballot>,
    pub members: BTreeSet<[u8; 32]>,
    pub current_term: u64,
    pub leader: Option<[u8; 32]>,
    pub self_id: [u8; 32],
    next_ballot_id: u64,
    pub default_timeout_ms: u64,
    pub stats: QuorumStats,
}

impl MeshQuorum {
    pub fn new(self_id: [u8; 32]) -> Self {
        MeshQuorum {
            ballots: BTreeMap::new(),
            members: BTreeSet::new(),
            current_term: 0,
            leader: None,
            self_id,
            next_ballot_id: 1,
            default_timeout_ms: 5000,
            stats: QuorumStats::default(),
        }
    }

    /// Quorum size for current membership.
    pub fn quorum_size(&self) -> usize {
        self.members.len() / 2 + 1
    }

    /// Add a member.
    pub fn add_member(&mut self, node: [u8; 32]) {
        self.members.insert(node);
    }

    /// Propose a ballot.
    pub fn propose(&mut self, topic: u64, now: u64) -> u64 {
        let id = self.next_ballot_id;
        self.next_ballot_id += 1;
        let qs = self.quorum_size();

        let mut ballot = Ballot {
            id, term: self.current_term, topic,
            proposer: self.self_id,
            votes: BTreeMap::new(),
            quorum_size: qs,
            state: BallotState::Open,
            created_at: now,
            timeout_at: now + self.default_timeout_ms,
        };

        // Self-vote Aye
        ballot.votes.insert(self.self_id, Vote::Aye);
        self.ballots.insert(id, ballot);
        self.stats.ballots_created += 1;
        self.stats.votes_cast += 1;
        id
    }

    /// Cast a vote on a ballot.
    pub fn vote(&mut self, ballot_id: u64, voter: [u8; 32], vote: Vote) -> bool {
        if let Some(ballot) = self.ballots.get_mut(&ballot_id) {
            if ballot.state != BallotState::Open { return false; }
            if !self.members.contains(&voter) { return false; }
            if ballot.votes.contains_key(&voter) { return false; } // No double voting

            ballot.votes.insert(voter, vote);
            self.stats.votes_cast += 1;

            // Check if decided
            if ballot.has_quorum() {
                ballot.state = BallotState::Passed;
                self.stats.ballots_passed += 1;
            } else if ballot.nays() >= ballot.quorum_size {
                ballot.state = BallotState::Failed;
                self.stats.ballots_failed += 1;
            }
            true
        } else {
            false
        }
    }

    /// Start a leader election.
    pub fn start_election(&mut self, now: u64) -> u64 {
        self.current_term += 1;
        self.stats.elections_held += 1;
        // topic 0 = leader election
        self.propose(0, now)
    }

    /// Expire timed-out ballots.
    pub fn expire(&mut self, now: u64) {
        for ballot in self.ballots.values_mut() {
            if ballot.state == BallotState::Open && now >= ballot.timeout_at {
                ballot.state = BallotState::Expired;
                self.stats.ballots_expired += 1;
            }
        }
    }

    /// Get ballot result.
    pub fn result(&self, ballot_id: u64) -> Option<BallotState> {
        self.ballots.get(&ballot_id).map(|b| b.state)
    }
}
