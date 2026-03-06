//! # Mesh Topology — Network Graph Manager
//!
//! Maintains a real-time view of the mesh network topology:
//! which nodes are connected, their link metrics, and the
//! shortest paths between them (Section 11.34).
//!
//! Features:
//! - Link-state topology graph
//! - Dijkstra shortest-path computation
//! - Multi-hop route tables
//! - Network partitioning detection
//! - Topology change notifications

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

/// A link between two mesh nodes.
#[derive(Debug, Clone)]
pub struct MeshLink {
    pub src: [u8; 32],
    pub dst: [u8; 32],
    pub latency_ms: u32,
    pub bandwidth_mbps: u32,
    pub hops: u8,
    pub alive: bool,
    pub last_seen: u64,
}

/// Topology statistics.
#[derive(Debug, Clone, Default)]
pub struct TopoStats {
    pub nodes: u64,
    pub links: u64,
    pub partitions: u64,
    pub route_recalculations: u64,
    pub topology_changes: u64,
}

/// A route entry (next-hop table).
#[derive(Debug, Clone)]
pub struct RouteEntry {
    pub destination: [u8; 32],
    pub next_hop: [u8; 32],
    pub cost: u32,         // Total latency
    pub hop_count: u8,
}

/// The Mesh Topology Manager.
pub struct MeshTopology {
    /// All known links
    pub links: Vec<MeshLink>,
    /// Adjacency list: node → set of neighbor node IDs
    pub adjacency: BTreeMap<[u8; 32], BTreeSet<[u8; 32]>>,
    /// Route table: destination → route
    pub routes: BTreeMap<[u8; 32], RouteEntry>,
    /// Our node ID
    pub self_id: [u8; 32],
    pub stats: TopoStats,
}

impl MeshTopology {
    pub fn new(self_id: [u8; 32]) -> Self {
        MeshTopology {
            links: Vec::new(),
            adjacency: BTreeMap::new(),
            routes: BTreeMap::new(),
            self_id,
            stats: TopoStats::default(),
        }
    }

    /// Add or update a link.
    pub fn update_link(&mut self, link: MeshLink) {
        // Remove old link between same endpoints
        self.links.retain(|l| !(l.src == link.src && l.dst == link.dst));

        // Update adjacency
        self.adjacency.entry(link.src).or_insert_with(BTreeSet::new).insert(link.dst);
        self.adjacency.entry(link.dst).or_insert_with(BTreeSet::new).insert(link.src);

        self.links.push(link);
        self.stats.topology_changes += 1;
        self.update_stats();
    }

    /// Remove a link (node disconnect).
    pub fn remove_link(&mut self, src: &[u8; 32], dst: &[u8; 32]) {
        self.links.retain(|l| !(l.src == *src && l.dst == *dst));
        if let Some(neighbors) = self.adjacency.get_mut(src) {
            neighbors.remove(dst);
        }
        if let Some(neighbors) = self.adjacency.get_mut(dst) {
            neighbors.remove(src);
        }
        self.stats.topology_changes += 1;
        self.update_stats();
    }

    /// Get link cost between two nodes.
    fn link_cost(&self, src: &[u8; 32], dst: &[u8; 32]) -> u32 {
        self.links.iter()
            .find(|l| l.src == *src && l.dst == *dst && l.alive)
            .map(|l| l.latency_ms)
            .unwrap_or(u32::MAX)
    }

    /// Recompute shortest paths from self using Dijkstra.
    pub fn recalculate_routes(&mut self) {
        self.routes.clear();
        let mut dist: BTreeMap<[u8; 32], u32> = BTreeMap::new();
        let mut prev: BTreeMap<[u8; 32], [u8; 32]> = BTreeMap::new();
        let mut visited: BTreeSet<[u8; 32]> = BTreeSet::new();

        dist.insert(self.self_id, 0);

        loop {
            // Find unvisited node with minimum distance
            let current = dist.iter()
                .filter(|(n, _)| !visited.contains(*n))
                .min_by_key(|(_, &d)| d)
                .map(|(n, d)| (*n, *d));

            let (node, node_dist) = match current {
                Some(v) => v,
                None => break,
            };

            visited.insert(node);

            // Relax neighbors
            if let Some(neighbors) = self.adjacency.get(&node).cloned() {
                for neighbor in neighbors {
                    let cost = self.link_cost(&node, &neighbor);
                    if cost == u32::MAX { continue; }
                    let new_dist = node_dist.saturating_add(cost);
                    let current_dist = dist.get(&neighbor).copied().unwrap_or(u32::MAX);
                    if new_dist < current_dist {
                        dist.insert(neighbor, new_dist);
                        prev.insert(neighbor, node);
                    }
                }
            }
        }

        // Build route table from prev map
        for (dest, &cost) in &dist {
            if *dest == self.self_id { continue; }
            // Trace back to find next-hop
            let mut hop = *dest;
            while let Some(&p) = prev.get(&hop) {
                if p == self.self_id { break; }
                hop = p;
            }
            let hop_count = self.count_hops(&prev, dest);
            self.routes.insert(*dest, RouteEntry {
                destination: *dest, next_hop: hop,
                cost, hop_count,
            });
        }

        self.stats.route_recalculations += 1;
    }

    /// Count hops from self to destination.
    fn count_hops(&self, prev: &BTreeMap<[u8; 32], [u8; 32]>, dest: &[u8; 32]) -> u8 {
        let mut count = 0u8;
        let mut node = *dest;
        while let Some(&p) = prev.get(&node) {
            count = count.saturating_add(1);
            if p == self.self_id { break; }
            node = p;
        }
        count
    }

    /// Get next hop for a destination.
    pub fn next_hop(&self, dest: &[u8; 32]) -> Option<&RouteEntry> {
        self.routes.get(dest)
    }

    /// Detect partitions (nodes unreachable from self).
    pub fn partitioned_nodes(&self) -> Vec<[u8; 32]> {
        let all_nodes: BTreeSet<[u8; 32]> = self.adjacency.keys().copied().collect();
        let reachable: BTreeSet<[u8; 32]> = self.routes.keys().copied().collect();
        all_nodes.difference(&reachable)
            .filter(|n| **n != self.self_id)
            .copied().collect()
    }

    fn update_stats(&mut self) {
        self.stats.nodes = self.adjacency.len() as u64;
        self.stats.links = self.links.len() as u64;
    }
}
