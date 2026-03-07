//! # Nexus Peer Reputation Engine
//!
//! Tracks and scores mesh peer behavior over time to
//! inform routing, resource allocation, and trust decisions
//! (Section 11.31).
//!
//! Features:
//! - Multi-dimensional scoring (uptime, bandwidth, latency, honesty)
//! - Decay over time (recent behavior weighted higher)
//! - Reputation thresholds for capability grants
//! - Blacklist/whitelist integration
//! - Sybil resistance (minimum service history required)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Reputation dimensions.
#[derive(Debug, Clone, Copy)]
pub struct ReputationScore {
    pub uptime: f32,
    pub bandwidth: f32,
    pub latency: f32,
    pub reliability: f32,
    pub honesty: f32,
}

impl ReputationScore {
    pub fn zero() -> Self {
        ReputationScore { uptime: 0.0, bandwidth: 0.0, latency: 0.0, reliability: 0.0, honesty: 0.0 }
    }

    /// Weighted composite score (0.0–1.0).
    pub fn composite(&self) -> f32 {
        (self.uptime * 0.15 + self.bandwidth * 0.20
            + self.latency * 0.15 + self.reliability * 0.30
            + self.honesty * 0.20)
            .max(0.0).min(1.0)
    }
}

/// A peer's reputation record.
#[derive(Debug, Clone)]
pub struct PeerReputation {
    pub peer_id: String,
    pub score: ReputationScore,
    pub events: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub blacklisted: bool,
    pub whitelisted: bool,
}

/// Reputation engine statistics.
#[derive(Debug, Clone, Default)]
pub struct ReputationStats {
    pub peers_tracked: u64,
    pub events_processed: u64,
    pub blacklists: u64,
    pub promotions: u64,
}

/// The Reputation Engine.
pub struct ReputationEngine {
    pub peers: BTreeMap<String, PeerReputation>,
    /// Minimum composite score to be considered "trusted"
    pub trust_threshold: f32,
    /// Minimum events before reputation is considered reliable
    pub min_events: u64,
    /// Decay factor per evaluation cycle (0.0–1.0)
    pub decay_factor: f32,
    pub stats: ReputationStats,
}

impl ReputationEngine {
    pub fn new() -> Self {
        ReputationEngine {
            peers: BTreeMap::new(),
            trust_threshold: 0.6,
            min_events: 10,
            decay_factor: 0.95,
            stats: ReputationStats::default(),
        }
    }

    /// Record a positive or negative event for a peer.
    pub fn record_event(
        &mut self, peer_id: &str, dimension: &str, value: f32, now: u64,
    ) {
        let peer = self.peers.entry(String::from(peer_id))
            .or_insert_with(|| {
                self.stats.peers_tracked += 1;
                PeerReputation {
                    peer_id: String::from(peer_id),
                    score: ReputationScore::zero(),
                    events: 0, first_seen: now, last_seen: now,
                    blacklisted: false, whitelisted: false,
                }
            });

        peer.last_seen = now;
        peer.events += 1;
        self.stats.events_processed += 1;

        // Exponential moving average
        let alpha = 0.2_f32;
        let val = value.max(0.0).min(1.0);
        match dimension {
            "uptime" => peer.score.uptime = peer.score.uptime * (1.0 - alpha) + val * alpha,
            "bandwidth" => peer.score.bandwidth = peer.score.bandwidth * (1.0 - alpha) + val * alpha,
            "latency" => peer.score.latency = peer.score.latency * (1.0 - alpha) + val * alpha,
            "reliability" => peer.score.reliability = peer.score.reliability * (1.0 - alpha) + val * alpha,
            "honesty" => peer.score.honesty = peer.score.honesty * (1.0 - alpha) + val * alpha,
            _ => {}
        }
    }

    /// Get composite reputation score for a peer.
    pub fn get_score(&self, peer_id: &str) -> Option<f32> {
        self.peers.get(peer_id).map(|p| p.score.composite())
    }

    /// Check if a peer is trusted.
    pub fn is_trusted(&self, peer_id: &str) -> bool {
        if let Some(peer) = self.peers.get(peer_id) {
            if peer.blacklisted { return false; }
            if peer.whitelisted { return true; }
            peer.events >= self.min_events && peer.score.composite() >= self.trust_threshold
        } else { false }
    }

    /// Blacklist a peer.
    pub fn blacklist(&mut self, peer_id: &str) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.blacklisted = true;
            self.stats.blacklists += 1;
        }
    }

    /// Apply time decay to all scores.
    pub fn decay_all(&mut self) {
        let factor = self.decay_factor;
        for peer in self.peers.values_mut() {
            peer.score.uptime *= factor;
            peer.score.bandwidth *= factor;
            peer.score.latency *= factor;
            peer.score.reliability *= factor;
            peer.score.honesty *= factor;
        }
    }

    /// Get top N peers by composite score.
    pub fn top_peers(&self, n: usize) -> Vec<(&str, f32)> {
        let mut scored: Vec<(&str, f32)> = self.peers.iter()
            .filter(|(_, p)| !p.blacklisted && p.events >= self.min_events)
            .map(|(id, p)| (id.as_str(), p.score.composite()))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        scored.truncate(n);
        scored
    }
}
