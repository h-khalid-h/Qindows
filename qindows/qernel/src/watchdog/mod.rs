//! # Qernel Watchdog Timer
//!
//! Hardware and software watchdog to detect kernel hangs,
//! driver deadlocks, and runaway Silos. If the watchdog is
//! not petted within the timeout, it triggers a system recovery.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Watchdog timeout in nanoseconds (default: 10 seconds).
static WATCHDOG_TIMEOUT_NS: AtomicU64 = AtomicU64::new(10_000_000_000);
/// Last pet timestamp (nanoseconds since boot).
static LAST_PET_NS: AtomicU64 = AtomicU64::new(0);
/// Is the watchdog enabled?
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Number of watchdog expirations.
static EXPIRATION_COUNT: AtomicU64 = AtomicU64::new(0);

/// Watchdog trigger action.
#[derive(Debug, Clone, Copy)]
pub enum WatchdogAction {
    /// Log a warning but continue
    Warn,
    /// Restart the offending subsystem
    RestartSubsystem,
    /// Trigger a kernel panic (with dump)
    Panic,
    /// Hard reset via ACPI
    HardReset,
    /// NMI to all cores (for debugging)
    Nmi,
}

/// Subsystem watchdog — monitors individual kernel subsystems.
#[derive(Debug, Clone)]
pub struct SubsystemWatchdog {
    /// Subsystem name
    pub name: &'static str,
    /// Is this subsystem alive?
    pub alive: bool,
    /// Last heartbeat (ns)
    pub last_heartbeat: u64,
    /// Timeout (ns)
    pub timeout_ns: u64,
    /// What to do on expiry
    pub action: WatchdogAction,
    /// Number of expirations
    pub expirations: u64,
    /// Max allowed expirations before escalation
    pub max_expirations: u64,
}

impl SubsystemWatchdog {
    pub fn new(name: &'static str, timeout_ms: u64, action: WatchdogAction) -> Self {
        SubsystemWatchdog {
            name,
            alive: true,
            last_heartbeat: 0,
            timeout_ns: timeout_ms * 1_000_000,
            action,
            expirations: 0,
            max_expirations: 3,
        }
    }

    /// Check if this watchdog has expired.
    pub fn check(&mut self, now_ns: u64) -> Option<WatchdogAction> {
        if !self.alive { return None; }

        // Guard against underflow (e.g. when last_heartbeat hasn't been set yet)
        if now_ns <= self.last_heartbeat { return None; }

        if now_ns - self.last_heartbeat > self.timeout_ns {
            self.expirations += 1;

            if self.expirations > self.max_expirations {
                // Escalate: subsystem is repeatedly dying
                return Some(WatchdogAction::Panic);
            }

            return Some(self.action);
        }

        None
    }

    /// Pet (heartbeat) this watchdog.
    pub fn pet(&mut self, now_ns: u64) {
        self.last_heartbeat = now_ns;
        self.expirations = 0;
    }
}

/// The Watchdog Manager.
pub struct WatchdogManager {
    /// Subsystem watchdogs
    pub subsystems: alloc::vec::Vec<SubsystemWatchdog>,
    /// Global action on system-level watchdog expiry
    pub global_action: WatchdogAction,
    /// Total checks performed
    pub total_checks: u64,
    /// Total expirations
    pub total_expirations: u64,
}

impl WatchdogManager {
    pub fn new() -> Self {
        let mut mgr = WatchdogManager {
            subsystems: alloc::vec::Vec::new(),
            global_action: WatchdogAction::Panic,
            total_checks: 0,
            total_expirations: 0,
        };

        // Register default subsystem watchdogs
        mgr.subsystems.push(SubsystemWatchdog::new(
            "scheduler", 5000, WatchdogAction::RestartSubsystem
        ));
        mgr.subsystems.push(SubsystemWatchdog::new(
            "sentinel", 10000, WatchdogAction::Warn
        ));
        mgr.subsystems.push(SubsystemWatchdog::new(
            "prism_journal", 30000, WatchdogAction::RestartSubsystem
        ));
        mgr.subsystems.push(SubsystemWatchdog::new(
            "aether_compositor", 2000, WatchdogAction::RestartSubsystem
        ));
        mgr.subsystems.push(SubsystemWatchdog::new(
            "nexus_mesh", 60000, WatchdogAction::Warn
        ));

        mgr
    }

    /// Run a check cycle — inspect all subsystem watchdogs.
    pub fn check_all(&mut self, now_ns: u64) -> alloc::vec::Vec<(&'static str, WatchdogAction)> {
        self.total_checks += 1;
        let mut triggers = alloc::vec::Vec::new();

        for wd in &mut self.subsystems {
            if let Some(action) = wd.check(now_ns) {
                self.total_expirations += 1;
                crate::serial_println!(
                    "WATCHDOG: {} expired! ({} expirations, action={:?})",
                    wd.name, wd.expirations, action
                );
                triggers.push((wd.name, action));
            }
        }

        triggers
    }

    /// Pet a specific subsystem watchdog.
    pub fn pet(&mut self, name: &str, now_ns: u64) {
        for wd in &mut self.subsystems {
            if wd.name == name {
                wd.pet(now_ns);
                return;
            }
        }
    }

    /// Register a new subsystem watchdog.
    pub fn register(&mut self, watchdog: SubsystemWatchdog) {
        self.subsystems.push(watchdog);
    }
}

/// Pet the global system watchdog.
pub fn pet(now_ns: u64) {
    LAST_PET_NS.store(now_ns, Ordering::Relaxed);
}

/// Enable the global watchdog.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    crate::serial_println!("[OK] System watchdog enabled (timeout: {}s)",
        WATCHDOG_TIMEOUT_NS.load(Ordering::Relaxed) / 1_000_000_000);
}

/// Check the global watchdog (called from NMI or timer).
pub fn check(now_ns: u64) -> bool {
    if !ENABLED.load(Ordering::Relaxed) { return false; }

    let last = LAST_PET_NS.load(Ordering::Relaxed);
    let timeout = WATCHDOG_TIMEOUT_NS.load(Ordering::Relaxed);

    if now_ns - last > timeout {
        EXPIRATION_COUNT.fetch_add(1, Ordering::Relaxed);
        true
    } else {
        false
    }
}
