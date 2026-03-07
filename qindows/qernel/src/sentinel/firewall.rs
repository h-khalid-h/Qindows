//! # Sentinel Network Firewall
//!
//! Per-Silo network access control. Every outbound connection
//! is evaluated against the Silo's capability tokens and the
//! system's firewall rules. Malicious patterns are blocked,
//! and Sentinel can quarantine a Silo that exhibits bad behavior.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Firewall action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Allow the connection
    Allow,
    /// Block silently
    Block,
    /// Block and log (for audit)
    BlockLog,
    /// Allow but rate-limit
    RateLimit(u32), // max bytes/sec
    /// Quarantine the entire Silo
    Quarantine,
}

/// Network protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Quic,
    QFabric, // Nexus mesh protocol
    Any,
}

/// IP address (simplified as 4-byte IPv4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpAddr {
    pub octets: [u8; 4],
}

impl IpAddr {
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        IpAddr { octets: [a, b, c, d] }
    }
    pub const LOCALHOST: IpAddr = IpAddr::new(127, 0, 0, 1);
    pub const ANY: IpAddr = IpAddr::new(0, 0, 0, 0);
    pub const BROADCAST: IpAddr = IpAddr::new(255, 255, 255, 255);

    /// Check if address matches a CIDR range.
    pub fn matches_cidr(&self, network: &IpAddr, prefix_len: u8) -> bool {
        let self_u32 = u32::from_be_bytes(self.octets);
        let net_u32 = u32::from_be_bytes(network.octets);
        let mask = if prefix_len >= 32 { !0u32 } else { !0u32 << (32 - prefix_len) };
        (self_u32 & mask) == (net_u32 & mask)
    }
}

/// Port range.
#[derive(Debug, Clone, Copy)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub const fn single(port: u16) -> Self { PortRange { start: port, end: port } }
    pub const fn range(start: u16, end: u16) -> Self { PortRange { start, end } }
    pub const ANY: PortRange = PortRange { start: 0, end: 65535 };

    pub fn contains(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct FirewallRule {
    /// Rule ID
    pub id: u64,
    /// Rule name (for display)
    pub name: String,
    /// Direction
    pub direction: Direction,
    /// Silo ID this rule applies to (None = system-wide)
    pub silo_id: Option<u64>,
    /// Protocol
    pub protocol: Protocol,
    /// Source address (with CIDR prefix)
    pub src_addr: Option<(IpAddr, u8)>,
    /// Destination address (with CIDR prefix)
    pub dst_addr: Option<(IpAddr, u8)>,
    /// Destination port range
    pub dst_port: Option<PortRange>,
    /// Action to take
    pub action: Action,
    /// Priority (lower = higher priority)
    pub priority: u32,
    /// Is this rule enabled?
    pub enabled: bool,
    /// Hit count
    pub hits: u64,
}

/// Connection direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

/// A connection attempt (to be evaluated against rules).
#[derive(Debug, Clone)]
pub struct ConnectionAttempt {
    pub silo_id: u64,
    pub direction: Direction,
    pub protocol: Protocol,
    pub src_addr: IpAddr,
    pub src_port: u16,
    pub dst_addr: IpAddr,
    pub dst_port: u16,
    pub timestamp: u64,
}

/// The Sentinel Firewall.
pub struct Firewall {
    /// Firewall rules (sorted by priority)
    pub rules: Vec<FirewallRule>,
    /// Quarantined Silos
    pub quarantined: Vec<u64>,
    /// Next rule ID
    next_id: u64,
    /// Connection log (circular buffer)
    pub connection_log: Vec<ConnectionLogEntry>,
    /// Max log entries
    pub max_log: usize,
    /// Default action (when no rule matches)
    pub default_action: Action,
    /// Statistics
    pub stats: FirewallStats,
}

/// Connection log entry.
#[derive(Debug, Clone)]
pub struct ConnectionLogEntry {
    pub attempt: ConnectionAttempt,
    pub action: Action,
    pub rule_id: Option<u64>,
}

/// Firewall statistics.
#[derive(Debug, Clone, Default)]
pub struct FirewallStats {
    pub total_evaluated: u64,
    pub allowed: u64,
    pub blocked: u64,
    pub rate_limited: u64,
    pub quarantines: u64,
}

impl Firewall {
    pub fn new() -> Self {
        let mut fw = Firewall {
            rules: Vec::new(),
            quarantined: Vec::new(),
            next_id: 1,
            connection_log: Vec::new(),
            max_log: 1000,
            default_action: Action::Allow,
            stats: FirewallStats::default(),
        };

        fw.register_defaults();
        fw
    }

    /// Register default security rules.
    fn register_defaults(&mut self) {
        // Allow localhost
        self.add_rule(FirewallRule {
            id: 0, name: String::from("Allow localhost"),
            direction: Direction::Both, silo_id: None,
            protocol: Protocol::Any,
            src_addr: Some((IpAddr::LOCALHOST, 32)),
            dst_addr: Some((IpAddr::LOCALHOST, 32)),
            dst_port: None,
            action: Action::Allow, priority: 0, enabled: true, hits: 0,
        });

        // Allow Nexus mesh (Q-Fabric)
        self.add_rule(FirewallRule {
            id: 0, name: String::from("Allow Q-Fabric mesh"),
            direction: Direction::Both, silo_id: None,
            protocol: Protocol::QFabric,
            src_addr: None, dst_addr: None, dst_port: None,
            action: Action::Allow, priority: 10, enabled: true, hits: 0,
        });

        // Block well-known malicious ports
        self.add_rule(FirewallRule {
            id: 0, name: String::from("Block SMB (445)"),
            direction: Direction::Inbound, silo_id: None,
            protocol: Protocol::Tcp,
            src_addr: None, dst_addr: None,
            dst_port: Some(PortRange::single(445)),
            action: Action::BlockLog, priority: 5, enabled: true, hits: 0,
        });

        self.add_rule(FirewallRule {
            id: 0, name: String::from("Block RDP (3389)"),
            direction: Direction::Inbound, silo_id: None,
            protocol: Protocol::Tcp,
            src_addr: None, dst_addr: None,
            dst_port: Some(PortRange::single(3389)),
            action: Action::BlockLog, priority: 5, enabled: true, hits: 0,
        });
    }

    /// Add a firewall rule.
    pub fn add_rule(&mut self, mut rule: FirewallRule) -> u64 {
        rule.id = self.next_id;
        self.next_id += 1;
        let id = rule.id;
        self.rules.push(rule);
        self.rules.sort_by_key(|r| r.priority);
        id
    }

    /// Evaluate a connection attempt.
    pub fn evaluate(&mut self, attempt: &ConnectionAttempt) -> Action {
        self.stats.total_evaluated += 1;

        // Check quarantine
        if self.quarantined.contains(&attempt.silo_id) {
            self.stats.blocked += 1;
            return Action::Block;
        }

        // Find first matching rule — collect info without borrowing self mutably
        let mut matched: Option<(usize, Action, u64)> = None;
        for (idx, rule) in self.rules.iter().enumerate() {
            if !rule.enabled { continue; }
            if !Self::rule_matches_static(rule, attempt) { continue; }
            matched = Some((idx, rule.action, rule.id));
            break;
        }

        if let Some((idx, action, rule_id)) = matched {
            self.rules[idx].hits += 1;

            // Log the connection
            self.log_connection(attempt, action, Some(rule_id));

            match action {
                Action::Allow => { self.stats.allowed += 1; }
                Action::Block | Action::BlockLog => { self.stats.blocked += 1; }
                Action::RateLimit(_) => { self.stats.rate_limited += 1; }
                Action::Quarantine => {
                    self.quarantined.push(attempt.silo_id);
                    self.stats.quarantines += 1;
                    self.stats.blocked += 1;
                }
            }

            return action;
        }

        // Default action
        self.log_connection(attempt, self.default_action, None);
        self.default_action
    }

    /// Check if a rule matches a connection attempt.
    fn rule_matches_static(rule: &FirewallRule, attempt: &ConnectionAttempt) -> bool {
        // Direction
        if rule.direction != Direction::Both && rule.direction != attempt.direction {
            return false;
        }

        // Silo
        if let Some(silo) = rule.silo_id {
            if silo != attempt.silo_id { return false; }
        }

        // Protocol
        if rule.protocol != Protocol::Any && rule.protocol != attempt.protocol {
            return false;
        }

        // Source address
        if let Some((ref addr, prefix)) = rule.src_addr {
            if !attempt.src_addr.matches_cidr(addr, prefix) { return false; }
        }

        // Destination address
        if let Some((ref addr, prefix)) = rule.dst_addr {
            if !attempt.dst_addr.matches_cidr(addr, prefix) { return false; }
        }

        // Destination port
        if let Some(ref port_range) = rule.dst_port {
            if !port_range.contains(attempt.dst_port) { return false; }
        }

        true
    }

    /// Log a connection.
    fn log_connection(&mut self, attempt: &ConnectionAttempt, action: Action, rule_id: Option<u64>) {
        if self.connection_log.len() >= self.max_log {
            self.connection_log.remove(0);
        }
        self.connection_log.push(ConnectionLogEntry {
            attempt: attempt.clone(),
            action,
            rule_id,
        });
    }

    /// Remove a Silo from quarantine.
    pub fn unquarantine(&mut self, silo_id: u64) {
        self.quarantined.retain(|&s| s != silo_id);
    }
}
