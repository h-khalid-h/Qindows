//! # Sentinel Firewall Traffic Bridge (Phase 149)
//!
//! ## Architecture Guardian: The Gap
//! `sentinel/firewall.rs` implements `Firewall`:
//! - `add_rule()` — inserts a `FirewallRule` into the rule table
//! - `Action::Block/BlockLog/Quarantine`
//! - `Direction::Outbound/Inbound/Both`
//!
//! **Missing link**: The firewall rule engine was never fed real traffic
//! decisions from `QTrafficLaw7Bridge`. Law 7 blocked Silos but the
//! Sentinel firewall rule table remained empty — no per-Silo deny rules
//! were ever inserted after quarantine verdicts.
//!
//! This module provides `SentinelFirewallBridge`:
//! 1. `block_silo_outbound()` — inserts BlockLog rule for quarantined Silo
//! 2. `quarantine_silo()` — full Firewall::Quarantine action
//! 3. `on_covert_channel()` — QTrafficEngine QuarantineCovert → rule insert

extern crate alloc;
use alloc::format;

use crate::sentinel::firewall::{Firewall, FirewallRule, Action, Protocol, IpAddr, Direction};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct FirewallBridgeStats {
    pub rules_inserted:    u64,
    pub silos_blocked:     u64,
    pub silos_quarantined: u64,
    pub covert_blocks:     u64,
}

// ── Sentinel Firewall Bridge ──────────────────────────────────────────────────

/// Feeds QTrafficEngine verdicts into the Sentinel Firewall rule table.
pub struct SentinelFirewallBridge {
    pub firewall: Firewall,
    pub stats:    FirewallBridgeStats,
}

impl SentinelFirewallBridge {
    pub fn new() -> Self {
        SentinelFirewallBridge {
            firewall: Firewall::new(),
            stats:    FirewallBridgeStats::default(),
        }
    }

    /// Block + log all outbound traffic from a Silo with a suspicious verdict.
    pub fn block_silo_outbound(&mut self, silo_id: u64) {
        self.stats.silos_blocked += 1;
        self.stats.rules_inserted += 1;

        let _rule_id = self.firewall.add_rule(FirewallRule {
            id: 0, // assigned by add_rule
            name: format!("silo_{}_block", silo_id),
            direction: Direction::Outbound,
            silo_id: Some(silo_id),
            protocol: Protocol::Any,
            src_addr: None,
            dst_addr: None,
            dst_port: None,
            action: Action::BlockLog,
            priority: 0,  // highest priority
            enabled: true,
            hits: 0,
        });

        crate::serial_println!(
            "[FW BRIDGE] Silo {} outbound BLOCKED (BlockLog rule inserted)", silo_id
        );
    }

    /// Full quarantine: Firewall::Quarantine action (widens to all traffic).
    pub fn quarantine_silo(&mut self, silo_id: u64) {
        self.stats.silos_quarantined += 1;
        self.stats.rules_inserted += 1;

        let _rule_id = self.firewall.add_rule(FirewallRule {
            id: 0,
            name: format!("silo_{}_quarantine", silo_id),
            direction: Direction::Both,
            silo_id: Some(silo_id),
            protocol: Protocol::Any,
            src_addr: None,
            dst_addr: None,
            dst_port: None,
            action: Action::Quarantine,
            priority: 0,
            enabled: true,
            hits: 0,
        });

        crate::serial_println!(
            "[FW BRIDGE] Silo {} QUARANTINED (full network isolation)", silo_id
        );
    }

    /// Called when QTrafficEngine detects a covert channel — quarantine + audit.
    pub fn on_covert_channel(&mut self, silo_id: u64) {
        self.stats.covert_blocks += 1;
        self.quarantine_silo(silo_id);
        crate::serial_println!(
            "[FW BRIDGE] Covert channel → Silo {} full quarantine", silo_id
        );
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FwBridge: rules={} blocked={} quarantined={} covert={}",
            self.stats.rules_inserted, self.stats.silos_blocked,
            self.stats.silos_quarantined, self.stats.covert_blocks
        );
    }
}
