//! # Nexus Packet Firewall
//!
//! Per-Silo, rule-based packet filtering for the Nexus networking
//! stack. Supports allow/deny rules, port ranges, protocol
//! matching, rate limiting, and connection tracking.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Network protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

/// Firewall rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
    Log,      // Allow + log
    RateLimit(u32), // Allow up to N packets/sec
}

/// Traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

/// IP address match.
#[derive(Debug, Clone)]
pub enum IpMatch {
    /// Any address
    Any,
    /// Exact IPv4
    Exact([u8; 4]),
    /// CIDR subnet (addr, prefix_len)
    Subnet([u8; 4], u8),
    /// Address range (start, end)
    Range([u8; 4], [u8; 4]),
}

impl IpMatch {
    pub fn matches(&self, addr: &[u8; 4]) -> bool {
        match self {
            IpMatch::Any => true,
            IpMatch::Exact(target) => addr == target,
            IpMatch::Subnet(network, prefix_len) => {
                let prefix = *prefix_len as u32;
                if prefix == 0 { return true; }
                if prefix >= 32 { return addr == network; }
                let mask = !((1u32 << (32 - prefix)) - 1);
                let addr_u32 = u32::from_be_bytes(*addr);
                let net_u32 = u32::from_be_bytes(*network);
                (addr_u32 & mask) == (net_u32 & mask)
            }
            IpMatch::Range(start, end) => {
                let a = u32::from_be_bytes(*addr);
                let s = u32::from_be_bytes(*start);
                let e = u32::from_be_bytes(*end);
                a >= s && a <= e
            }
        }
    }
}

/// Port match.
#[derive(Debug, Clone)]
pub enum PortMatch {
    Any,
    Exact(u16),
    Range(u16, u16),
    List(Vec<u16>),
}

impl PortMatch {
    pub fn matches(&self, port: u16) -> bool {
        match self {
            PortMatch::Any => true,
            PortMatch::Exact(p) => port == *p,
            PortMatch::Range(start, end) => port >= *start && port <= *end,
            PortMatch::List(ports) => ports.contains(&port),
        }
    }
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct FirewallRule {
    /// Rule ID
    pub id: u32,
    /// Rule name
    pub name: String,
    /// Priority (lower = evaluated first)
    pub priority: u16,
    /// Direction
    pub direction: Direction,
    /// Protocol
    pub protocol: Protocol,
    /// Source IP match
    pub src_ip: IpMatch,
    /// Destination IP match
    pub dst_ip: IpMatch,
    /// Source port match
    pub src_port: PortMatch,
    /// Destination port match
    pub dst_port: PortMatch,
    /// Silo ID (0 = applies to all)
    pub silo_id: u64,
    /// Action
    pub action: Action,
    /// Is this rule enabled?
    pub enabled: bool,
    /// Hit count
    pub hits: u64,
}

/// A packet to be evaluated.
#[derive(Debug, Clone)]
pub struct Packet {
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: Protocol,
    pub silo_id: u64,
    pub direction: Direction,
    pub size: u32,
}

/// Firewall evaluation result.
#[derive(Debug, Clone, Copy)]
pub enum Verdict {
    Accept,
    Drop,
    RateLimit,
}

/// Firewall statistics.
#[derive(Debug, Clone, Default)]
pub struct FwStats {
    pub packets_evaluated: u64,
    pub packets_allowed: u64,
    pub packets_denied: u64,
    pub packets_logged: u64,
    pub packets_rate_limited: u64,
    pub no_rule_matched: u64,
}

/// The Packet Firewall.
pub struct Firewall {
    /// Rules (sorted by priority)
    pub rules: Vec<FirewallRule>,
    /// Default action when no rule matches
    pub default_action: Action,
    /// Next rule ID
    next_id: u32,
    /// Stats
    pub stats: FwStats,
    /// Enable logging
    pub logging: bool,
}

impl Firewall {
    pub fn new(default: Action) -> Self {
        Firewall {
            rules: Vec::new(),
            default_action: default,
            next_id: 1,
            stats: FwStats::default(),
            logging: false,
        }
    }

    /// Add a rule.
    pub fn add_rule(&mut self, mut rule: FirewallRule) -> u32 {
        rule.id = self.next_id;
        self.next_id += 1;
        let id = rule.id;

        // Insert sorted by priority
        let pos = self.rules.iter().position(|r| r.priority > rule.priority)
            .unwrap_or(self.rules.len());
        self.rules.insert(pos, rule);

        id
    }

    /// Remove a rule by ID.
    pub fn remove_rule(&mut self, id: u32) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < before
    }

    /// Evaluate a packet against the rules.
    pub fn evaluate(&mut self, packet: &Packet) -> Verdict {
        self.stats.packets_evaluated += 1;

        for rule in &mut self.rules {
            if !rule.enabled { continue; }

            // Direction match
            if rule.direction != Direction::Both && rule.direction != packet.direction {
                continue;
            }

            // Protocol match
            if rule.protocol != Protocol::Any && rule.protocol != packet.protocol {
                continue;
            }

            // Silo match
            if rule.silo_id != 0 && rule.silo_id != packet.silo_id {
                continue;
            }

            // IP matches
            if !rule.src_ip.matches(&packet.src_ip) { continue; }
            if !rule.dst_ip.matches(&packet.dst_ip) { continue; }

            // Port matches
            if !rule.src_port.matches(packet.src_port) { continue; }
            if !rule.dst_port.matches(packet.dst_port) { continue; }

            // Rule matched!
            rule.hits += 1;

            return match rule.action {
                Action::Allow => {
                    self.stats.packets_allowed += 1;
                    Verdict::Accept
                }
                Action::Deny => {
                    self.stats.packets_denied += 1;
                    Verdict::Drop
                }
                Action::Log => {
                    self.stats.packets_logged += 1;
                    self.stats.packets_allowed += 1;
                    Verdict::Accept
                }
                Action::RateLimit(_) => {
                    self.stats.packets_rate_limited += 1;
                    Verdict::RateLimit
                }
            };
        }

        // No rule matched — apply default
        self.stats.no_rule_matched += 1;
        match self.default_action {
            Action::Allow | Action::Log => {
                self.stats.packets_allowed += 1;
                Verdict::Accept
            }
            _ => {
                self.stats.packets_denied += 1;
                Verdict::Drop
            }
        }
    }

    /// Shorthand: allow all traffic from a Silo.
    pub fn allow_silo(&mut self, silo_id: u64, name: &str) -> u32 {
        self.add_rule(FirewallRule {
            id: 0, name: String::from(name), priority: 100,
            direction: Direction::Both, protocol: Protocol::Any,
            src_ip: IpMatch::Any, dst_ip: IpMatch::Any,
            src_port: PortMatch::Any, dst_port: PortMatch::Any,
            silo_id, action: Action::Allow, enabled: true, hits: 0,
        })
    }

    /// Shorthand: block a destination port.
    pub fn block_port(&mut self, port: u16, proto: Protocol, name: &str) -> u32 {
        self.add_rule(FirewallRule {
            id: 0, name: String::from(name), priority: 50,
            direction: Direction::Outbound, protocol: proto,
            src_ip: IpMatch::Any, dst_ip: IpMatch::Any,
            src_port: PortMatch::Any, dst_port: PortMatch::Exact(port),
            silo_id: 0, action: Action::Deny, enabled: true, hits: 0,
        })
    }
}
