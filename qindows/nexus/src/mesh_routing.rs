//! # Nexus Mesh Routing
//!
//! Distributed routing for the Global Mesh (Q-Fabric).
//! Each Qindows node maintains a routing table of nearby peers,
//! enabling multi-hop relay, latency-based path selection, and
//! automatic failover when links degrade.
//!
//! Supports:
//! - Distance-vector routing (simplified RIP-like)
//! - Latency-weighted path selection
//! - Multi-path load balancing
//! - Automatic peer discovery via mDNS/DHT

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── Node Identity ──────────────────────────────────────────────────────────

/// A mesh node identifier (256-bit public key hash).
pub type NodeId = [u8; 32];

/// A mesh node's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Online and reachable
    Online,
    /// Reachable but degraded (high latency / packet loss)
    Degraded,
    /// Unreachable (timeout)
    Unreachable,
    /// Banned by Sentinel
    Banned,
}

/// Information about a peer node.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Node ID
    pub id: NodeId,
    /// Human-readable name
    pub name: String,
    /// IPv4 address
    pub addr: [u8; 4],
    /// Port
    pub port: u16,
    /// Status
    pub status: NodeStatus,
    /// Last seen (ns since boot)
    pub last_seen: u64,
    /// Round-trip time (ms)
    pub rtt_ms: u32,
    /// Packet loss percentage (0–100)
    pub packet_loss: u8,
    /// Available bandwidth (bytes/sec)
    pub bandwidth: u64,
    /// Trust score (0–100, from Sentinel reputation)
    pub trust_score: u8,
}

// ─── Routing Table ──────────────────────────────────────────────────────────

/// A route to a destination node.
#[derive(Debug, Clone)]
pub struct Route {
    /// Destination node
    pub dest: NodeId,
    /// Next-hop node (where to send packets)
    pub next_hop: NodeId,
    /// Hop count
    pub hops: u8,
    /// Estimated latency (ms)
    pub latency_ms: u32,
    /// Estimated bandwidth (bytes/sec)
    pub bandwidth: u64,
    /// Route metric (lower = better)
    pub metric: u32,
    /// When this route was last updated (ns)
    pub updated_at: u64,
    /// Is this route actively in use?
    pub active: bool,
}

/// Route selection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStrategy {
    /// Lowest latency
    LowestLatency,
    /// Lowest hop count
    FewestHops,
    /// Highest bandwidth
    HighestBandwidth,
    /// Weighted combination
    Balanced,
}

// ─── Mesh Router ────────────────────────────────────────────────────────────

/// Router statistics.
#[derive(Debug, Clone, Default)]
pub struct RouterStats {
    pub routes_learned: u64,
    pub routes_expired: u64,
    pub packets_forwarded: u64,
    pub packets_dropped: u64,
    pub route_updates_received: u64,
    pub route_updates_sent: u64,
    pub failovers: u64,
}

/// The Mesh Router.
pub struct MeshRouter {
    /// This node's ID
    pub self_id: NodeId,
    /// Direct peers (1-hop neighbors)
    pub peers: BTreeMap<NodeId, PeerInfo>,
    /// Full routing table (multi-hop included)
    pub routes: Vec<Route>,
    /// Route selection strategy
    pub strategy: RouteStrategy,
    /// Route expiry timeout (ns)
    pub route_timeout_ns: u64,
    /// Maximum hop count
    pub max_hops: u8,
    /// Statistics
    pub stats: RouterStats,
}

impl MeshRouter {
    pub fn new(self_id: NodeId) -> Self {
        MeshRouter {
            self_id,
            peers: BTreeMap::new(),
            routes: Vec::new(),
            strategy: RouteStrategy::Balanced,
            route_timeout_ns: 30_000_000_000, // 30 seconds
            max_hops: 8,
            stats: RouterStats::default(),
        }
    }

    /// Add or update a direct peer.
    pub fn update_peer(&mut self, peer: PeerInfo, now: u64) {
        let id = peer.id;

        // Add direct route for this peer
        let metric = self.compute_metric(1, peer.rtt_ms, peer.bandwidth, peer.trust_score);
        let route = Route {
            dest: id,
            next_hop: id,
            hops: 1,
            latency_ms: peer.rtt_ms,
            bandwidth: peer.bandwidth,
            metric,
            updated_at: now,
            active: peer.status == NodeStatus::Online,
        };

        // Update or insert route
        if let Some(existing) = self.routes.iter_mut().find(|r| r.dest == id && r.hops == 1) {
            *existing = route;
        } else {
            self.routes.push(route);
            self.stats.routes_learned += 1;
        }

        self.peers.insert(id, peer);
    }

    /// Process a routing update from a peer.
    pub fn process_update(&mut self, from: NodeId, remote_routes: &[Route], now: u64) {
        self.stats.route_updates_received += 1;

        let from_peer = match self.peers.get(&from) {
            Some(p) => p.clone(),
            None => return, // Unknown peer — ignore
        };

        for remote in remote_routes {
            // Skip routes back to ourselves
            if remote.dest == self.self_id { continue; }

            // Don't exceed max hops
            let new_hops = remote.hops.saturating_add(1);
            if new_hops > self.max_hops { continue; }

            let new_latency = remote.latency_ms.saturating_add(from_peer.rtt_ms);
            let new_bandwidth = remote.bandwidth.min(from_peer.bandwidth);
            let new_metric = self.compute_metric(
                new_hops, new_latency, new_bandwidth, from_peer.trust_score,
            );

            // Check if we already have a better route
            let dominated = self.routes.iter().any(|r| {
                r.dest == remote.dest && r.active && r.metric <= new_metric
            });

            if !dominated {
                let route = Route {
                    dest: remote.dest,
                    next_hop: from,
                    hops: new_hops,
                    latency_ms: new_latency,
                    bandwidth: new_bandwidth,
                    metric: new_metric,
                    updated_at: now,
                    active: true,
                };

                // Replace or add
                if let Some(existing) = self.routes.iter_mut()
                    .find(|r| r.dest == remote.dest && r.next_hop == from)
                {
                    *existing = route;
                } else {
                    self.routes.push(route);
                    self.stats.routes_learned += 1;
                }
            }
        }
    }

    /// Find the best route to a destination.
    pub fn lookup(&self, dest: &NodeId) -> Option<&Route> {
        self.routes.iter()
            .filter(|r| &r.dest == dest && r.active)
            .min_by_key(|r| r.metric)
    }

    /// Find all routes to a destination (for multi-path).
    pub fn lookup_all(&self, dest: &NodeId) -> Vec<&Route> {
        let mut routes: Vec<&Route> = self.routes.iter()
            .filter(|r| &r.dest == dest && r.active)
            .collect();
        routes.sort_by_key(|r| r.metric);
        routes
    }

    /// Expire stale routes.
    pub fn expire_routes(&mut self, now: u64) {
        for route in &mut self.routes {
            if route.active && now.saturating_sub(route.updated_at) > self.route_timeout_ns {
                route.active = false;
                self.stats.routes_expired += 1;
            }
        }

        // Detect peers that are no longer reachable
        for peer in self.peers.values_mut() {
            if now.saturating_sub(peer.last_seen) > self.route_timeout_ns * 3 {
                peer.status = NodeStatus::Unreachable;
            }
        }
    }

    /// Compute route metric (lower = better).
    fn compute_metric(&self, hops: u8, latency_ms: u32, bandwidth: u64, trust: u8) -> u32 {
        match self.strategy {
            RouteStrategy::LowestLatency => latency_ms,
            RouteStrategy::FewestHops => hops as u32 * 1000,
            RouteStrategy::HighestBandwidth => {
                // Invert bandwidth so lower metric = higher BW
                if bandwidth == 0 { u32::MAX } else {
                    (1_000_000_000u64 / bandwidth).min(u32::MAX as u64) as u32
                }
            }
            RouteStrategy::Balanced => {
                // Weighted combination
                let hop_cost = hops as u32 * 100;
                let latency_cost = latency_ms;
                let bw_cost = if bandwidth > 0 {
                    (100_000_000u64 / bandwidth).min(1000) as u32
                } else {
                    1000
                };
                let trust_penalty = (100u32.saturating_sub(trust as u32)) * 5;
                hop_cost + latency_cost + bw_cost + trust_penalty
            }
        }
    }

    /// Get online peer count.
    pub fn online_peers(&self) -> usize {
        self.peers.values().filter(|p| p.status == NodeStatus::Online).count()
    }

    /// Get reachable destinations count.
    pub fn reachable_destinations(&self) -> usize {
        let mut unique: Vec<NodeId> = self.routes.iter()
            .filter(|r| r.active)
            .map(|r| r.dest)
            .collect();
        unique.sort();
        unique.dedup();
        unique.len()
    }
}
