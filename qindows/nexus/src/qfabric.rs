//! # Q-Fabric — Transport Aggregation Layer
//!
//! Bonds multiple network interfaces (WiFi, 5G, Ethernet, Mesh Radio)
//! into a single logical pipe. The application sees one fast, reliable
//! connection — Q-Fabric decides which physical link carries each packet.
//!
//! Key features (Section 5 of the spec):
//! - **Multi-path**: Spread traffic across all available links
//! - **Failover**: If WiFi drops, 5G takes over seamlessly
//! - **Latency steering**: Gaming traffic → lowest latency link
//! - **Bandwidth aggregation**: Download at WiFi + 5G combined speed
//! - **Per-Silo routing**: Each app can prefer a different link

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Physical link type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkType {
    Ethernet,
    WiFi,
    Cellular5G,
    Cellular4G,
    MeshRadio,
    Loopback,
}

impl LinkType {
    /// Typical latency (ms).
    pub fn typical_latency(&self) -> u32 {
        match self {
            LinkType::Ethernet => 1,
            LinkType::WiFi => 5,
            LinkType::Cellular5G => 15,
            LinkType::Cellular4G => 40,
            LinkType::MeshRadio => 50,
            LinkType::Loopback => 0,
        }
    }
}

/// Link state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
    Degraded,
    Connecting,
}

/// A physical network link.
#[derive(Debug, Clone)]
pub struct PhysicalLink {
    /// Link ID
    pub id: u64,
    /// Interface name (e.g., "eth0", "wlan0")
    pub name: String,
    /// Link type
    pub link_type: LinkType,
    /// Current state
    pub state: LinkState,
    /// Measured bandwidth (bytes/sec)
    pub bandwidth: u64,
    /// Measured latency (ms)
    pub latency_ms: u32,
    /// Packet loss percentage (0–100)
    pub packet_loss: u8,
    /// Bytes sent
    pub tx_bytes: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Is metered (e.g., cellular — avoid for bulk transfers)
    pub metered: bool,
    /// Signal strength (0–100, for wireless)
    pub signal_strength: u8,
    /// Weight for load balancing (higher = more traffic)
    pub weight: u32,
}

/// Traffic class — determines which link to prefer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficClass {
    /// Real-time (VoIP, gaming) — lowest latency link
    RealTime,
    /// Interactive (browsing, API calls) — balanced
    Interactive,
    /// Bulk (downloads, sync) — highest bandwidth
    Bulk,
    /// Background (updates, telemetry) — cheapest link
    Background,
}

/// Bond mode — how traffic is distributed across links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondMode {
    /// Round-robin across all links
    RoundRobin,
    /// Active-backup (one link active, others standby)
    ActiveBackup,
    /// Weighted balance (proportional to link weight)
    Weighted,
    /// Traffic-class aware (latency vs bandwidth)
    ClassAware,
}

/// A flow — a logical connection being routed through the fabric.
#[derive(Debug, Clone)]
pub struct Flow {
    /// Flow ID
    pub id: u64,
    /// Source Silo
    pub silo_id: u64,
    /// Traffic class
    pub traffic_class: TrafficClass,
    /// Assigned link ID
    pub assigned_link: u64,
    /// Bytes transferred
    pub bytes: u64,
}

/// Q-Fabric statistics.
#[derive(Debug, Clone, Default)]
pub struct FabricStats {
    pub total_tx: u64,
    pub total_rx: u64,
    pub failovers: u64,
    pub flows_created: u64,
    pub rebalances: u64,
}

/// The Q-Fabric Transport Aggregator.
pub struct QFabric {
    /// Physical links
    pub links: BTreeMap<u64, PhysicalLink>,
    /// Active flows
    pub flows: BTreeMap<u64, Flow>,
    /// Bond mode
    pub mode: BondMode,
    /// Next flow ID
    next_flow_id: u64,
    /// Round-robin index
    rr_index: usize,
    /// Statistics
    pub stats: FabricStats,
}

impl QFabric {
    pub fn new(mode: BondMode) -> Self {
        QFabric {
            links: BTreeMap::new(),
            flows: BTreeMap::new(),
            mode,
            next_flow_id: 1,
            rr_index: 0,
            stats: FabricStats::default(),
        }
    }

    /// Add a physical link.
    pub fn add_link(&mut self, link: PhysicalLink) {
        self.links.insert(link.id, link);
    }

    /// Remove a link (triggers failover).
    pub fn remove_link(&mut self, link_id: u64) {
        self.links.remove(&link_id);
        // Reassign flows from this link
        let orphaned: Vec<(u64, TrafficClass)> = self.flows.iter()
            .filter(|(_, f)| f.assigned_link == link_id)
            .map(|(&id, f)| (id, f.traffic_class))
            .collect();
        for (fid, tc) in orphaned {
            if let Some(new_link) = self.select_link(tc) {
                if let Some(flow) = self.flows.get_mut(&fid) {
                    flow.assigned_link = new_link;
                    self.stats.failovers += 1;
                }
            }
        }
    }

    /// Create a new flow.
    pub fn create_flow(&mut self, silo_id: u64, traffic_class: TrafficClass) -> Option<u64> {
        let link = self.select_link(traffic_class)?;
        let id = self.next_flow_id;
        self.next_flow_id += 1;

        self.flows.insert(id, Flow {
            id,
            silo_id,
            traffic_class,
            assigned_link: link,
            bytes: 0,
        });
        self.stats.flows_created += 1;
        Some(id)
    }

    /// Send data on a flow.
    pub fn send(&mut self, flow_id: u64, bytes: u64) -> Result<u64, &'static str> {
        let flow = self.flows.get_mut(&flow_id).ok_or("Flow not found")?;
        let link_id = flow.assigned_link;
        flow.bytes = flow.bytes.saturating_add(bytes);

        let link = self.links.get_mut(&link_id).ok_or("Link down")?;
        if link.state != LinkState::Up {
            return Err("Link not available");
        }
        link.tx_bytes = link.tx_bytes.saturating_add(bytes);
        self.stats.total_tx = self.stats.total_tx.saturating_add(bytes);
        Ok(link_id)
    }

    /// Select the best link for a traffic class.
    fn select_link(&mut self, traffic_class: TrafficClass) -> Option<u64> {
        let up_links: Vec<&PhysicalLink> = self.links.values()
            .filter(|l| l.state == LinkState::Up)
            .collect();

        if up_links.is_empty() { return None; }

        match self.mode {
            BondMode::RoundRobin => {
                self.rr_index = (self.rr_index + 1) % up_links.len();
                Some(up_links[self.rr_index].id)
            }
            BondMode::ActiveBackup => {
                Some(up_links[0].id)
            }
            BondMode::Weighted => {
                up_links.iter().max_by_key(|l| l.weight).map(|l| l.id)
            }
            BondMode::ClassAware => {
                match traffic_class {
                    TrafficClass::RealTime => {
                        // Lowest latency, non-metered preferred
                        up_links.iter()
                            .filter(|l| !l.metered)
                            .min_by_key(|l| l.latency_ms)
                            .or_else(|| up_links.iter().min_by_key(|l| l.latency_ms))
                            .map(|l| l.id)
                    }
                    TrafficClass::Interactive => {
                        up_links.iter()
                            .min_by_key(|l| l.latency_ms as u64 * 2 + (100 - l.signal_strength as u64))
                            .map(|l| l.id)
                    }
                    TrafficClass::Bulk => {
                        // Highest bandwidth, non-metered
                        up_links.iter()
                            .filter(|l| !l.metered)
                            .max_by_key(|l| l.bandwidth)
                            .or_else(|| up_links.iter().max_by_key(|l| l.bandwidth))
                            .map(|l| l.id)
                    }
                    TrafficClass::Background => {
                        // Cheapest (non-metered, lowest signal OK)
                        up_links.iter()
                            .filter(|l| !l.metered)
                            .min_by_key(|l| l.bandwidth) // Use slowest free link
                            .or_else(|| up_links.iter().min_by_key(|l| l.bandwidth))
                            .map(|l| l.id)
                    }
                }
            }
        }
    }

    /// Rebalance flows across links.
    pub fn rebalance(&mut self) {
        let flow_ids: Vec<u64> = self.flows.keys().copied().collect();
        for fid in flow_ids {
            if let Some(flow) = self.flows.get(&fid) {
                let tc = flow.traffic_class;
                if let Some(better) = self.select_link(tc) {
                    if let Some(flow) = self.flows.get_mut(&fid) {
                        if flow.assigned_link != better {
                            flow.assigned_link = better;
                            self.stats.rebalances += 1;
                        }
                    }
                }
            }
        }
    }

    /// Get aggregate bandwidth across all up links.
    pub fn aggregate_bandwidth(&self) -> u64 {
        self.links.values()
            .filter(|l| l.state == LinkState::Up)
            .map(|l| l.bandwidth)
            .sum()
    }

    /// Get lowest latency across all up links.
    pub fn best_latency(&self) -> u32 {
        self.links.values()
            .filter(|l| l.state == LinkState::Up)
            .map(|l| l.latency_ms)
            .min()
            .unwrap_or(u32::MAX)
    }
}
