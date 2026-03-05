//! # Watchdog Timer — Hang Detection & Recovery
//!
//! Detects unresponsive subsystems and forces recovery
//! actions (Section 12.2). Each subsystem must "pet" the
//! watchdog periodically; failure to do so triggers escalation.
//!
//! Escalation ladder:
//! 1. Log warning + nudge subsystem
//! 2. Force-restart the subsystem
//! 3. Isolate and failover to replica
//! 4. Emergency Silo kill

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Watchdog state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WdState {
    Healthy,
    Warning,
    Restarting,
    Isolated,
    Dead,
}

/// Escalation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Escalation {
    None = 0,
    Warn = 1,
    Restart = 2,
    Isolate = 3,
    Kill = 4,
}

/// A watched subsystem.
#[derive(Debug, Clone)]
pub struct WatchedSystem {
    pub id: u64,
    pub name: String,
    pub silo_id: u64,
    pub state: WdState,
    pub timeout_ms: u64,
    pub last_pet: u64,
    pub escalation: Escalation,
    pub restarts: u32,
    pub max_restarts: u32,
}

/// Watchdog statistics.
#[derive(Debug, Clone, Default)]
pub struct WdStats {
    pub systems_watched: u64,
    pub warnings: u64,
    pub restarts: u64,
    pub isolations: u64,
    pub kills: u64,
    pub pets_received: u64,
}

/// The Watchdog Timer.
pub struct Watchdog {
    pub systems: BTreeMap<u64, WatchedSystem>,
    next_id: u64,
    pub stats: WdStats,
}

impl Watchdog {
    pub fn new() -> Self {
        Watchdog {
            systems: BTreeMap::new(),
            next_id: 1,
            stats: WdStats::default(),
        }
    }

    /// Register a subsystem to watch.
    pub fn watch(&mut self, name: &str, silo_id: u64, timeout_ms: u64, max_restarts: u32, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.systems.insert(id, WatchedSystem {
            id, name: String::from(name), silo_id,
            state: WdState::Healthy, timeout_ms,
            last_pet: now, escalation: Escalation::None,
            restarts: 0, max_restarts,
        });

        self.stats.systems_watched += 1;
        id
    }

    /// Pet (heartbeat) from a subsystem.
    pub fn pet(&mut self, id: u64, now: u64) {
        if let Some(sys) = self.systems.get_mut(&id) {
            sys.last_pet = now;
            if sys.state == WdState::Warning {
                sys.state = WdState::Healthy;
                sys.escalation = Escalation::None;
            }
            self.stats.pets_received += 1;
        }
    }

    /// Check all systems for timeouts and escalate.
    pub fn tick(&mut self, now: u64) -> Vec<(u64, Escalation)> {
        let mut actions = Vec::new();

        let ids: Vec<u64> = self.systems.keys().copied().collect();
        for id in ids {
            let sys = match self.systems.get_mut(&id) {
                Some(s) => s,
                None => continue,
            };

            if sys.state == WdState::Dead || sys.state == WdState::Isolated {
                continue;
            }

            let elapsed = now.saturating_sub(sys.last_pet);
            if elapsed <= sys.timeout_ms {
                continue;
            }

            // Escalate
            let new_esc = match sys.escalation {
                Escalation::None => {
                    sys.state = WdState::Warning;
                    self.stats.warnings += 1;
                    Escalation::Warn
                }
                Escalation::Warn => {
                    if sys.restarts < sys.max_restarts {
                        sys.state = WdState::Restarting;
                        sys.restarts += 1;
                        self.stats.restarts += 1;
                        Escalation::Restart
                    } else {
                        sys.state = WdState::Isolated;
                        self.stats.isolations += 1;
                        Escalation::Isolate
                    }
                }
                Escalation::Restart => {
                    sys.state = WdState::Isolated;
                    self.stats.isolations += 1;
                    Escalation::Isolate
                }
                Escalation::Isolate => {
                    sys.state = WdState::Dead;
                    self.stats.kills += 1;
                    Escalation::Kill
                }
                Escalation::Kill => continue,
            };

            sys.escalation = new_esc;
            actions.push((id, new_esc));
        }

        actions
    }

    /// Unwatch a subsystem.
    pub fn unwatch(&mut self, id: u64) {
        self.systems.remove(&id);
    }
}
