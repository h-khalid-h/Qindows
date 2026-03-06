//! # Mesh Discovery — Zero-Configuration Node Discovery
//!
//! Discovers mesh peers on the local network using multicast
//! announcements (Section 11.22).
//!
//! Features:
//! - Multicast announce/listen
//! - Service advertisement (capabilities, version)
//! - Node fingerprinting (public key, region)
//! - TTL-based expiry for stale nodes
//! - Deduplication of announcements

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A discovered peer node.
#[derive(Debug, Clone)]
pub struct DiscoveredNode {
    pub node_id: [u8; 32],
    pub address: [u8; 4],
    pub port: u16,
    pub version: u32,
    pub capabilities: u64,
    pub region: String,
    pub discovered_at: u64,
    pub last_announce: u64,
    pub announce_count: u32,
}

/// Announcement message.
#[derive(Debug, Clone)]
pub struct Announcement {
    pub node_id: [u8; 32],
    pub address: [u8; 4],
    pub port: u16,
    pub version: u32,
    pub capabilities: u64,
    pub region: String,
    pub ttl_ms: u64,
}

/// Discovery statistics.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    pub announcements_sent: u64,
    pub announcements_received: u64,
    pub nodes_discovered: u64,
    pub nodes_expired: u64,
    pub duplicates_ignored: u64,
}

/// The Mesh Discovery Engine.
pub struct MeshDiscovery {
    pub self_id: [u8; 32],
    pub peers: BTreeMap<[u8; 32], DiscoveredNode>,
    pub multicast_addr: [u8; 4],
    pub multicast_port: u16,
    pub announce_interval_ms: u64,
    pub ttl_ms: u64,
    pub stats: DiscoveryStats,
}

impl MeshDiscovery {
    pub fn new(self_id: [u8; 32]) -> Self {
        MeshDiscovery {
            self_id,
            peers: BTreeMap::new(),
            multicast_addr: [239, 255, 81, 1], // 239.255.81.1 (Q=81)
            multicast_port: 51800,
            announce_interval_ms: 10_000,
            ttl_ms: 30_000,
            stats: DiscoveryStats::default(),
        }
    }

    /// Generate our announcement.
    pub fn create_announcement(&mut self, address: [u8; 4], port: u16, version: u32, capabilities: u64, region: &str) -> Announcement {
        self.stats.announcements_sent += 1;
        Announcement {
            node_id: self.self_id, address, port, version,
            capabilities, region: String::from(region), ttl_ms: self.ttl_ms,
        }
    }

    /// Process a received announcement.
    pub fn on_announcement(&mut self, ann: Announcement, now: u64) {
        self.stats.announcements_received += 1;

        // Ignore self
        if ann.node_id == self.self_id { return; }

        if let Some(existing) = self.peers.get_mut(&ann.node_id) {
            existing.last_announce = now;
            existing.announce_count += 1;
            existing.address = ann.address;
            existing.port = ann.port;
            existing.capabilities = ann.capabilities;
            self.stats.duplicates_ignored += 1;
        } else {
            self.peers.insert(ann.node_id, DiscoveredNode {
                node_id: ann.node_id, address: ann.address,
                port: ann.port, version: ann.version,
                capabilities: ann.capabilities, region: ann.region,
                discovered_at: now, last_announce: now, announce_count: 1,
            });
            self.stats.nodes_discovered += 1;
        }
    }

    /// Expire stale peers.
    pub fn expire(&mut self, now: u64) {
        let expired: Vec<[u8; 32]> = self.peers.values()
            .filter(|p| now.saturating_sub(p.last_announce) > self.ttl_ms)
            .map(|p| p.node_id)
            .collect();
        for id in expired {
            self.peers.remove(&id);
            self.stats.nodes_expired += 1;
        }
    }

    /// Get active peer count.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}
