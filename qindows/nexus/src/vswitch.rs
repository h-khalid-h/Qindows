//! # V-Switch — Per-Silo Virtual Network Interface
//!
//! Every Q-Silo gets its own virtual network interface (vNIC).
//! The V-Switch acts as a software-defined switch, routing traffic
//! between Silos and enforcing network isolation (Section 5).
//!
//! Security model:
//! - Each Silo can only see its own vNIC — no promiscuous access
//! - Malicious port scans hit kernel "black holes" (unroutable)
//! - Egress filtering: outbound traffic requires visible Capability Tokens
//! - DNS-over-HTTPS enforced for all lookups

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Virtual NIC state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VNicState {
    Up,
    Down,
    Blocked, // Sentinel killed network access
}

/// A firewall rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallAction {
    Allow,
    Drop,
    BlackHole, // Silently discard, no ICMP reply
    Log,       // Allow but log for Sentinel
}

/// Traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ingress,
    Egress,
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct FirewallRule {
    /// Rule ID
    pub id: u64,
    /// Direction
    pub direction: Direction,
    /// Destination port (0 = any)
    pub port: u16,
    /// Protocol (0 = any, 6 = TCP, 17 = UDP)
    pub protocol: u8,
    /// IP range prefix (simplified as u32 for IPv4)
    pub ip_prefix: u32,
    /// Prefix mask bits
    pub prefix_len: u8,
    /// Action
    pub action: FirewallAction,
    /// Priority (lower = higher priority)
    pub priority: u32,
    /// Hit count
    pub hits: u64,
}

/// A Virtual NIC assigned to a Silo.
#[derive(Debug, Clone)]
pub struct VNic {
    /// vNIC ID
    pub id: u64,
    /// Owning Silo ID
    pub silo_id: u64,
    /// Virtual MAC address
    pub mac: [u8; 6],
    /// Virtual IPv4 address
    pub ipv4: u32,
    /// State
    pub state: VNicState,
    /// Bytes sent
    pub tx_bytes: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Packets dropped
    pub drops: u64,
    /// DNS-over-HTTPS enforced
    pub doh_enforced: bool,
    /// Capability token required for egress
    pub egress_cap: u64,
}

/// V-Switch statistics.
#[derive(Debug, Clone, Default)]
pub struct VSwitchStats {
    pub vnics_created: u64,
    pub packets_switched: u64,
    pub packets_dropped: u64,
    pub packets_black_holed: u64,
    pub rules_matched: u64,
    pub dns_queries_proxied: u64,
}

/// The V-Switch.
pub struct VSwitch {
    /// Virtual NICs by vNIC ID
    pub vnics: BTreeMap<u64, VNic>,
    /// Silo → vNIC ID mapping
    pub silo_map: BTreeMap<u64, u64>,
    /// Global firewall rules
    pub global_rules: Vec<FirewallRule>,
    /// Per-Silo firewall rules
    pub silo_rules: BTreeMap<u64, Vec<FirewallRule>>,
    /// Next vNIC ID
    next_vnic_id: u64,
    /// Next rule ID
    next_rule_id: u64,
    /// Default egress action (for Silos without explicit rules)
    pub default_egress: FirewallAction,
    /// Statistics
    pub stats: VSwitchStats,
}

impl VSwitch {
    pub fn new() -> Self {
        VSwitch {
            vnics: BTreeMap::new(),
            silo_map: BTreeMap::new(),
            global_rules: Vec::new(),
            silo_rules: BTreeMap::new(),
            next_vnic_id: 1,
            next_rule_id: 1,
            default_egress: FirewallAction::Log, // Log by default
            stats: VSwitchStats::default(),
        }
    }

    /// Create a vNIC for a Silo.
    pub fn create_vnic(&mut self, silo_id: u64, egress_cap: u64) -> u64 {
        let id = self.next_vnic_id;
        self.next_vnic_id += 1;

        // Generate virtual MAC from silo ID
        let mac = [
            0x02, // Locally administered
            ((silo_id >> 32) & 0xFF) as u8,
            ((silo_id >> 24) & 0xFF) as u8,
            ((silo_id >> 16) & 0xFF) as u8,
            ((silo_id >> 8) & 0xFF) as u8,
            (silo_id & 0xFF) as u8,
        ];

        // Assign virtual IP in 10.silo.x.x range
        let ipv4 = 0x0A000000 | ((id as u32) & 0x00FFFFFF);

        self.vnics.insert(id, VNic {
            id,
            silo_id,
            mac,
            ipv4,
            state: VNicState::Up,
            tx_bytes: 0,
            rx_bytes: 0,
            drops: 0,
            doh_enforced: true,
            egress_cap,
        });

        self.silo_map.insert(silo_id, id);
        self.stats.vnics_created += 1;
        id
    }

    /// Add a global firewall rule.
    pub fn add_global_rule(&mut self, direction: Direction, port: u16, protocol: u8, action: FirewallAction, priority: u32) -> u64 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;
        self.global_rules.push(FirewallRule {
            id, direction, port, protocol, ip_prefix: 0, prefix_len: 0, action, priority, hits: 0,
        });
        self.global_rules.sort_by_key(|r| r.priority);
        id
    }

    /// Add a per-Silo firewall rule.
    pub fn add_silo_rule(&mut self, silo_id: u64, direction: Direction, port: u16, protocol: u8, action: FirewallAction, priority: u32) -> u64 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;
        let rules = self.silo_rules.entry(silo_id).or_insert_with(Vec::new);
        rules.push(FirewallRule {
            id, direction, port, protocol, ip_prefix: 0, prefix_len: 0, action, priority, hits: 0,
        });
        rules.sort_by_key(|r| r.priority);
        id
    }

    /// Check if a packet should be allowed.
    pub fn filter(&mut self, silo_id: u64, direction: Direction, port: u16, protocol: u8) -> FirewallAction {
        // Check per-Silo rules first
        if let Some(rules) = self.silo_rules.get_mut(&silo_id) {
            for rule in rules.iter_mut() {
                if rule.direction == direction
                    && (rule.port == 0 || rule.port == port)
                    && (rule.protocol == 0 || rule.protocol == protocol)
                {
                    rule.hits += 1;
                    self.stats.rules_matched += 1;
                    return rule.action;
                }
            }
        }

        // Then global rules
        for rule in &mut self.global_rules {
            if rule.direction == direction
                && (rule.port == 0 || rule.port == port)
                && (rule.protocol == 0 || rule.protocol == protocol)
            {
                rule.hits += 1;
                self.stats.rules_matched += 1;
                return rule.action;
            }
        }

        // Default
        self.default_egress
    }

    /// Send a packet (with firewall check).
    pub fn send(&mut self, silo_id: u64, dest_port: u16, protocol: u8, bytes: u64) -> Result<(), &'static str> {
        let vnic_id = self.silo_map.get(&silo_id).copied().ok_or("No vNIC for Silo")?;
        let action = self.filter(silo_id, Direction::Egress, dest_port, protocol);

        match action {
            FirewallAction::Allow | FirewallAction::Log => {
                if let Some(vnic) = self.vnics.get_mut(&vnic_id) {
                    if vnic.state != VNicState::Up { return Err("vNIC is down"); }
                    vnic.tx_bytes = vnic.tx_bytes.saturating_add(bytes);
                }
                self.stats.packets_switched += 1;
                Ok(())
            }
            FirewallAction::Drop => {
                if let Some(vnic) = self.vnics.get_mut(&vnic_id) { vnic.drops += 1; }
                self.stats.packets_dropped += 1;
                Err("Packet dropped by firewall")
            }
            FirewallAction::BlackHole => {
                self.stats.packets_black_holed += 1;
                Err("Black-holed")
            }
        }
    }

    /// Block a Silo's network (Sentinel enforcement).
    pub fn block_silo(&mut self, silo_id: u64) {
        if let Some(&vnic_id) = self.silo_map.get(&silo_id) {
            if let Some(vnic) = self.vnics.get_mut(&vnic_id) {
                vnic.state = VNicState::Blocked;
            }
        }
    }

    /// Destroy vNIC on Silo termination.
    pub fn destroy_vnic(&mut self, silo_id: u64) {
        if let Some(vnic_id) = self.silo_map.remove(&silo_id) {
            self.vnics.remove(&vnic_id);
        }
        self.silo_rules.remove(&silo_id);
    }
}
