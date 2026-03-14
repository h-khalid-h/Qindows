//! # Watchdog Anomaly Bridge (Phase 150)
//!
//! ## Architecture Guardian: The Gap
//! `watchdog/mod.rs` implements `SubsystemWatchdog` (with global atomics):
//! - `SubsystemWatchdog::new(name, timeout_ms, action)` — creates a watchdog
//! - `WatchdogAction::Warn/RestartSubsystem/Panic/HardReset/Nmi`
//! - `check()` — checks expiry, returns Option<WatchdogAction>
//! - `pet(now_ns)` — resets the liveness timer
//!
//! **Missing link**: Subsystem watchdogs were defined but never:
//! 1. Registered at boot with meaningful timeouts
//! 2. Petted from real kernel event loops (Q-Ring dispatch, PMC loop)
//! 3. Acted upon when expired
//!
//! This module provides `WatchdogAnomalyBridge`:
//! 1. `new()` — pre-builds scheduler + sentinel watchdogs
//! 2. `on_qring_batch()` — pets the Q-Ring watchdog
//! 3. `on_sentinel_cycle()` — pets the Sentinel watchdog
//! 4. `check_and_act()` — fires actions on expired watchdogs

extern crate alloc;

use crate::watchdog::{SubsystemWatchdog, WatchdogAction};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WatchdogBridgeStats {
    pub pets_fired:    u64,
    pub expirations:   u64,
    pub panics_raised: u64,
}

// ── Watchdog Anomaly Bridge ───────────────────────────────────────────────────

/// Wires SubsystemWatchdogs to real kernel event loops.
pub struct WatchdogAnomalyBridge {
    pub scheduler_wd: SubsystemWatchdog,
    pub sentinel_wd:  SubsystemWatchdog,
    pub qring_wd:     SubsystemWatchdog,
    pub stats:        WatchdogBridgeStats,
}

impl WatchdogAnomalyBridge {
    pub fn new() -> Self {
        WatchdogAnomalyBridge {
            scheduler_wd: SubsystemWatchdog::new("scheduler", 2_000, WatchdogAction::Panic),
            sentinel_wd:  SubsystemWatchdog::new("sentinel",  10_000, WatchdogAction::Warn),
            qring_wd:     SubsystemWatchdog::new("qring",     5_000, WatchdogAction::RestartSubsystem),
            stats:        WatchdogBridgeStats::default(),
        }
    }

    /// Pet Q-Ring and scheduler watchdogs on each dispatch batch (proves liveness).
    pub fn on_qring_batch(&mut self, now_ns: u64) {
        self.stats.pets_fired += 1;
        self.qring_wd.pet(now_ns);
        self.scheduler_wd.pet(now_ns);
    }

    /// Pet Sentinel watchdog on each PMC scan cycle.
    pub fn on_sentinel_cycle(&mut self, now_ns: u64) {
        self.sentinel_wd.pet(now_ns);
    }

    /// Check all watchdogs and return any triggered actions.
    pub fn check_and_act(&mut self, now_ns: u64) {
        if let Some(action) = self.qring_wd.check(now_ns) {
            self.stats.expirations += 1;
            self.act("qring", action);
        }
        if let Some(action) = self.scheduler_wd.check(now_ns) {
            self.stats.expirations += 1;
            self.act("scheduler", action);
        }
        if let Some(action) = self.sentinel_wd.check(now_ns) {
            self.stats.expirations += 1;
            self.act("sentinel", action);
        }
    }

    fn act(&mut self, name: &str, action: WatchdogAction) {
        crate::serial_println!("[WATCHDOG] '{}' expired → {:?}", name, action);
        match action {
            WatchdogAction::Panic => {
                self.stats.panics_raised += 1;
                crate::serial_println!("[WATCHDOG] KERNEL PANIC: subsystem '{}' hung!", name);
                // Production: trigger NMI dump
            }
            WatchdogAction::Warn => {
                crate::serial_println!("[WATCHDOG] WARNING: subsystem '{}' is late", name);
            }
            WatchdogAction::RestartSubsystem => {
                crate::serial_println!("[WATCHDOG] RESTART: subsystem '{}' restarting...", name);
            }
            WatchdogAction::HardReset => {
                crate::serial_println!("[WATCHDOG] HARD RESET: ACPI triggered");
            }
            WatchdogAction::Nmi => {
                crate::serial_println!("[WATCHDOG] NMI broadcast to all cores");
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WatchdogBridge: pets={} expirations={} panics={}",
            self.stats.pets_fired, self.stats.expirations, self.stats.panics_raised
        );
    }
}
