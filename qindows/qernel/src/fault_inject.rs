//! # Fault Injection — Chaos-Testing Subsystem
//!
//! Deliberately injects faults into subsystems to verify
//! resilience under adverse conditions (Section 12.1).
//!
//! Fault types:
//! - Memory allocation failures
//! - Disk I/O errors
//! - Network packet drops
//! - Timer drift
//! - NPU computation errors
//! - Simulated thermal spikes

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultType {
    AllocFailure,
    DiskError,
    NetworkDrop,
    TimerDrift,
    NpuError,
    ThermalSpike,
    CapabilityRevoke,
}

/// Injection trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Inject after N operations
    AfterCount(u64),
    /// Inject with probability (0-100%)
    Probability(u8),
    /// Inject once at timestamp
    AtTime(u64),
    /// Always inject
    Always,
}

/// Fault injection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjState {
    Armed,
    Fired,
    Expired,
    Disabled,
}

/// A fault injection rule.
#[derive(Debug, Clone)]
pub struct FaultRule {
    pub id: u64,
    pub fault_type: FaultType,
    pub trigger: Trigger,
    pub target_subsystem: String,
    pub state: InjState,
    pub fire_count: u64,
    pub max_fires: u64,
    pub created_at: u64,
}

/// Fault injection statistics.
#[derive(Debug, Clone, Default)]
pub struct FaultStats {
    pub rules_created: u64,
    pub faults_injected: u64,
    pub subsystems_affected: u64,
    pub rules_expired: u64,
}

/// The Fault Injection Engine.
pub struct FaultInjector {
    pub rules: BTreeMap<u64, FaultRule>,
    next_id: u64,
    /// Operation counters per subsystem
    pub op_counters: BTreeMap<String, u64>,
    /// PRNG state for probability triggers
    rng_state: u64,
    pub stats: FaultStats,
}

impl FaultInjector {
    pub fn new() -> Self {
        FaultInjector {
            rules: BTreeMap::new(),
            next_id: 1,
            op_counters: BTreeMap::new(),
            rng_state: 0xDEAD_BEEF_CAFE_BABEu64,
            stats: FaultStats::default(),
        }
    }

    /// Create a fault injection rule.
    pub fn arm(&mut self, fault_type: FaultType, trigger: Trigger, subsystem: &str, max_fires: u64, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.rules.insert(id, FaultRule {
            id, fault_type, trigger,
            target_subsystem: String::from(subsystem),
            state: InjState::Armed,
            fire_count: 0, max_fires, created_at: now,
        });

        self.stats.rules_created += 1;
        id
    }

    /// Check if a fault should be injected for the given subsystem.
    pub fn check(&mut self, subsystem: &str, now: u64) -> Option<FaultType> {
        let counter = self.op_counters.entry(String::from(subsystem)).or_insert(0);
        *counter += 1;
        let count = *counter;

        let mut fired_rule_id = None;

        for rule in self.rules.values() {
            if rule.state != InjState::Armed { continue; }
            if rule.target_subsystem != subsystem { continue; }

            let should_fire = match rule.trigger {
                Trigger::AfterCount(n) => count >= n,
                Trigger::Probability(pct) => {
                    let roll = self.pseudo_random() % 100;
                    roll < pct as u64
                }
                Trigger::AtTime(t) => now >= t,
                Trigger::Always => true,
            };

            if should_fire {
                fired_rule_id = Some((rule.id, rule.fault_type));
                break;
            }
        }

        if let Some((id, fault_type)) = fired_rule_id {
            if let Some(rule) = self.rules.get_mut(&id) {
                rule.fire_count += 1;
                if rule.fire_count >= rule.max_fires {
                    rule.state = InjState::Expired;
                    self.stats.rules_expired += 1;
                } else {
                    rule.state = InjState::Fired;
                }
            }
            self.stats.faults_injected += 1;
            return Some(fault_type);
        }

        None
    }

    /// Reset a fired rule back to armed.
    pub fn rearm(&mut self, id: u64) {
        if let Some(rule) = self.rules.get_mut(&id) {
            if rule.state == InjState::Fired {
                rule.state = InjState::Armed;
            }
        }
    }

    /// Disable a rule.
    pub fn disable(&mut self, id: u64) {
        if let Some(rule) = self.rules.get_mut(&id) {
            rule.state = InjState::Disabled;
        }
    }

    /// Simple PRNG (xoshiro-style).
    fn pseudo_random(&mut self) -> u64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        self.rng_state
    }
}
