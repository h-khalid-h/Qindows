//! # Silo Firewall — Per-Silo Network Packet Filtering
//!
//! Each Silo gets its own firewall ruleset, isolated from
//! others (Section 11.8). Rules can filter by protocol,
//! port, IP, and direction.
//!
//! Features:
//! - Per-Silo rulesets (independent firewall per Silo)
//! - L3/L4 filtering (IP, TCP/UDP port, ICMP)
//! - Default deny with explicit allow rules
//! - Connection tracking for stateful filtering
//! - Rate limiting

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Packet direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// Protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Any,
}

/// Firewall action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwAction {
    Allow,
    Deny,
    RateLimit(u32), // max packets/sec
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct FwRule {
    pub id: u64,
    pub silo_id: u64,
    pub direction: Direction,
    pub protocol: Protocol,
    pub src_ip: u32,
    pub src_mask: u32,
    pub dst_ip: u32,
    pub dst_mask: u32,
    pub dst_port: u16,
    pub action: FwAction,
    pub priority: u32,
    pub hit_count: u64,
}

/// A tracked connection.
#[derive(Debug, Clone)]
pub struct ConnTrack {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: Protocol,
    pub packets: u64,
    pub last_seen: u64,
}

/// Firewall statistics.
#[derive(Debug, Clone, Default)]
pub struct FwStats {
    pub packets_allowed: u64,
    pub packets_denied: u64,
    pub packets_rate_limited: u64,
    pub rules_evaluated: u64,
    pub connections_tracked: u64,
}

/// The Silo Firewall.
pub struct SiloFirewall {
    pub rules: BTreeMap<u64, Vec<FwRule>>,  // silo → rules
    pub conn_table: Vec<ConnTrack>,
    pub default_action: FwAction,
    next_rule_id: u64,
    pub max_connections: usize,
    pub stats: FwStats,
}

impl SiloFirewall {
    pub fn new() -> Self {
        SiloFirewall {
            rules: BTreeMap::new(),
            conn_table: Vec::new(),
            default_action: FwAction::Deny,
            next_rule_id: 1,
            max_connections: 10000,
            stats: FwStats::default(),
        }
    }

    /// Add a firewall rule.
    pub fn add_rule(&mut self, silo_id: u64, direction: Direction, protocol: Protocol,
                    src_ip: u32, src_mask: u32, dst_ip: u32, dst_mask: u32,
                    dst_port: u16, action: FwAction, priority: u32) -> u64 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;

        let ruleset = self.rules.entry(silo_id).or_insert_with(Vec::new);
        ruleset.push(FwRule {
            id, silo_id, direction, protocol,
            src_ip, src_mask, dst_ip, dst_mask,
            dst_port, action, priority, hit_count: 0,
        });
        ruleset.sort_by(|a, b| b.priority.cmp(&a.priority));
        id
    }

    /// Evaluate a packet against rules.
    pub fn evaluate(&mut self, silo_id: u64, direction: Direction, protocol: Protocol,
                    src_ip: u32, dst_ip: u32, dst_port: u16) -> FwAction {
        let ruleset = match self.rules.get_mut(&silo_id) {
            Some(r) => r,
            None => {
                match self.default_action {
                    FwAction::Allow => self.stats.packets_allowed += 1,
                    FwAction::Deny => self.stats.packets_denied += 1,
                    _ => {}
                }
                return self.default_action;
            }
        };

        for rule in ruleset.iter_mut() {
            self.stats.rules_evaluated += 1;

            if rule.direction != direction {
                continue;
            }
            if rule.protocol != Protocol::Any && rule.protocol != protocol {
                continue;
            }
            if rule.src_mask != 0 && (src_ip & rule.src_mask) != (rule.src_ip & rule.src_mask) {
                continue;
            }
            if rule.dst_mask != 0 && (dst_ip & rule.dst_mask) != (rule.dst_ip & rule.dst_mask) {
                continue;
            }
            if rule.dst_port != 0 && rule.dst_port != dst_port {
                continue;
            }

            // Match found
            rule.hit_count += 1;
            match rule.action {
                FwAction::Allow => self.stats.packets_allowed += 1,
                FwAction::Deny => self.stats.packets_denied += 1,
                FwAction::RateLimit(_) => self.stats.packets_rate_limited += 1,
            }
            return rule.action;
        }

        // Default action
        match self.default_action {
            FwAction::Allow => self.stats.packets_allowed += 1,
            FwAction::Deny => self.stats.packets_denied += 1,
            _ => {}
        }
        self.default_action
    }

    /// Remove a rule.
    pub fn remove_rule(&mut self, silo_id: u64, rule_id: u64) {
        if let Some(ruleset) = self.rules.get_mut(&silo_id) {
            ruleset.retain(|r| r.id != rule_id);
        }
    }
}
