//! # Nexus Peer Reputation System
//!
//! Tracks the reliability and trustworthiness of peers on the Global Mesh.
//! Peers earn reputation through successful interactions and lose it
//! through failures, timeouts, and malicious behavior.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A peer's reputation score.
#[derive(Debug, Clone)]
pub struct PeerReputation {
    /// Peer node ID
    pub node_id: [u8; 32],
    /// Reputation score (0.0 - 100.0)
    pub score: f32,
    /// Trust level
    pub trust_level: TrustLevel,
    /// Successful interactions
    pub successes: u64,
    /// Failed interactions
    pub failures: u64,
    /// Bytes transferred successfully
    pub bytes_transferred: u64,
    /// Average response time (ms)
    pub avg_response_ms: u32,
    /// Last interaction timestamp
    pub last_seen: u64,
    /// First seen timestamp
    pub first_seen: u64,
    /// Is this peer currently banned?
    pub banned: bool,
    /// Ban reason
    pub ban_reason: Option<String>,
    /// Number of reports against this peer
    pub reports: u32,
}

/// Trust levels derived from reputation score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    /// Brand new peer (score 0-20)
    Unknown,
    /// Some interactions (20-40)
    Low,
    /// Moderate history (40-60)
    Medium,
    /// Good track record (60-80)
    High,
    /// Excellent long-term peer (80-100)
    Trusted,
}

impl TrustLevel {
    pub fn from_score(score: f32) -> Self {
        match score as u32 {
            0..=19 => TrustLevel::Unknown,
            20..=39 => TrustLevel::Low,
            40..=59 => TrustLevel::Medium,
            60..=79 => TrustLevel::High,
            _ => TrustLevel::Trusted,
        }
    }
}

/// Events that affect reputation.
#[derive(Debug, Clone, Copy)]
pub enum ReputationEvent {
    /// Successful data transfer
    SuccessfulTransfer { bytes: u64, response_ms: u32 },
    /// Failed to respond in time
    Timeout,
    /// Returned corrupted data
    CorruptData,
    /// Connection reset unexpectedly
    ConnectionReset,
    /// Peer sent malicious data
    MaliciousBehavior,
    /// Peer relayed data for us
    RelayService,
    /// Peer provided valid DHT response
    DhtResponse,
    /// Peer reported by another trusted peer
    ReportedByPeer,
}

/// The Reputation Manager.
pub struct ReputationManager {
    /// Per-peer reputation data
    pub peers: BTreeMap<[u8; 32], PeerReputation>,
    /// Score change weights
    pub weights: ScoreWeights,
    /// Minimum score before auto-ban
    pub ban_threshold: f32,
    /// Stats
    pub stats: RepStats,
}

/// Weights for different reputation events.
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    pub success: f32,
    pub timeout: f32,
    pub corrupt_data: f32,
    pub connection_reset: f32,
    pub malicious: f32,
    pub relay_bonus: f32,
    pub dht_response: f32,
    pub report_penalty: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        ScoreWeights {
            success: 0.5,
            timeout: -2.0,
            corrupt_data: -10.0,
            connection_reset: -1.0,
            malicious: -50.0,
            relay_bonus: 1.0,
            dht_response: 0.3,
            report_penalty: -5.0,
        }
    }
}

/// Reputation statistics.
#[derive(Debug, Clone, Default)]
pub struct RepStats {
    pub total_events: u64,
    pub peers_tracked: u64,
    pub peers_banned: u64,
    pub peers_promoted: u64,
}

impl ReputationManager {
    pub fn new() -> Self {
        ReputationManager {
            peers: BTreeMap::new(),
            weights: ScoreWeights::default(),
            ban_threshold: 5.0,
            stats: RepStats::default(),
        }
    }

    /// Record a reputation event for a peer.
    pub fn record_event(&mut self, node_id: [u8; 32], event: ReputationEvent, now: u64) {
        self.stats.total_events += 1;

        let peer = self.peers.entry(node_id).or_insert_with(|| {
            self.stats.peers_tracked += 1;
            PeerReputation {
                node_id,
                score: 50.0, // Start at neutral
                trust_level: TrustLevel::Medium,
                successes: 0,
                failures: 0,
                bytes_transferred: 0,
                avg_response_ms: 0,
                last_seen: now,
                first_seen: now,
                banned: false,
                ban_reason: None,
                reports: 0,
            }
        });

        peer.last_seen = now;

        let score_delta = match event {
            ReputationEvent::SuccessfulTransfer { bytes, response_ms } => {
                peer.successes += 1;
                peer.bytes_transferred += bytes;
                // Running average for response time
                peer.avg_response_ms = ((peer.avg_response_ms as u64 * (peer.successes - 1)
                    + response_ms as u64) / peer.successes) as u32;
                self.weights.success
            }
            ReputationEvent::Timeout => {
                peer.failures += 1;
                self.weights.timeout
            }
            ReputationEvent::CorruptData => {
                peer.failures += 1;
                self.weights.corrupt_data
            }
            ReputationEvent::ConnectionReset => {
                peer.failures += 1;
                self.weights.connection_reset
            }
            ReputationEvent::MaliciousBehavior => {
                peer.failures += 1;
                self.weights.malicious
            }
            ReputationEvent::RelayService => {
                peer.successes += 1;
                self.weights.relay_bonus
            }
            ReputationEvent::DhtResponse => {
                peer.successes += 1;
                self.weights.dht_response
            }
            ReputationEvent::ReportedByPeer => {
                peer.reports += 1;
                self.weights.report_penalty
            }
        };

        // Apply score change (clamped to 0-100)
        peer.score = (peer.score + score_delta).max(0.0).min(100.0);

        // Update trust level
        let old_level = peer.trust_level;
        peer.trust_level = TrustLevel::from_score(peer.score);

        if peer.trust_level > old_level {
            self.stats.peers_promoted += 1;
        }

        // Auto-ban
        if peer.score <= self.ban_threshold && !peer.banned {
            peer.banned = true;
            peer.ban_reason = Some(String::from("Score below threshold"));
            self.stats.peers_banned += 1;
        }
    }

    /// Get a peer's trust level.
    pub fn trust_level(&self, node_id: &[u8; 32]) -> TrustLevel {
        self.peers.get(node_id)
            .map(|p| p.trust_level)
            .unwrap_or(TrustLevel::Unknown)
    }

    /// Is a peer banned?
    pub fn is_banned(&self, node_id: &[u8; 32]) -> bool {
        self.peers.get(node_id).map(|p| p.banned).unwrap_or(false)
    }

    /// Get top N most trusted peers.
    pub fn top_peers(&self, n: usize) -> Vec<&PeerReputation> {
        let mut peers: Vec<&PeerReputation> = self.peers.values()
            .filter(|p| !p.banned)
            .collect();
        peers.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(core::cmp::Ordering::Equal));
        peers.truncate(n);
        peers
    }

    /// Unban a peer (manual override).
    pub fn unban(&mut self, node_id: &[u8; 32]) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.banned = false;
            peer.ban_reason = None;
            peer.score = 20.0; // Reset to low
        }
    }

    /// Decay scores for inactive peers (call periodically).
    pub fn decay(&mut self, now: u64, inactive_threshold_ns: u64) {
        for peer in self.peers.values_mut() {
            if now - peer.last_seen > inactive_threshold_ns {
                peer.score = (peer.score * 0.99).max(0.0); // Slow decay
            }
        }
    }
}
