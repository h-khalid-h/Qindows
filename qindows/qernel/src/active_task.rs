//! # Active Task Token — Law 8 Energy Proportionality Enforcement (Phase 73)
//!
//! Q-Manifest **Law 8: Energy Proportionality**
//! > "Background Silos without Active Task token → Fibers deep-sleep; violators throttled"
//!
//! ## ARCHITECTURE.md §Q-MANIFEST Law 8
//! A Silo must hold an **Active Task CapToken** to run Fibers at full clock speed.
//! Without it, the Sentinel signals the scheduler to deep-sleep all Fibers in that Silo.
//! This prevents the "25 background apps draining battery" problem from Windows/macOS.
//!
//! ## Architecture Guardian: Layering
//! ```text
//! App Silo                         Qernel
//! ────────────────────────────     ──────────────────────────────────────────
//! Q-Ring: RequestActiveTask  ───►  ActiveTaskManager::grant(silo, reason, duration)
//!                                       │ Issues ActiveTaskToken
//!                                       │ Sentinel: unthrottle Silo Fibers
//! Q-Ring: ReleaseActiveTask  ───►  ActiveTaskManager::release(silo)
//!                                       │ Sentinel: deep-sleep all Silo Fibers
//! ```
//!
//! ## Why this matters for the benchmarks
//! ARCHITECTURE.md: "RAM (Idle): ~450 MB" — only possible because background Silos
//! don't hold full working sets in RAM. Deep-sleeping Fibers swap their stacks to NVMe.
//!
//! ## Core Types
//! - `ActiveTaskToken` — a time-limited capability to run at full speed
//! - `TaskCategory` — why the Silo needs to run (Foreground, Audio, Sync, Timer)
//! - `ActiveTaskManager` — kernel registry of all current active-task grants

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Task Categories ───────────────────────────────────────────────────────────

/// Why a Silo needs to run at full power. Controls priority and token duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCategory {
    /// User is actively interacting (foreground window focus)
    UserForeground,
    /// Real-time audio rendering (must never skip)
    AudioRealtime,
    /// Background data synchronization (limited duration)
    BackgroundSync,
    /// One-shot timer callback (strictly time-bounded)
    TimerCallback,
    /// Critical system task (Sentinel, boot wizard, update)
    SystemCritical,
    /// AI/NPU inference task (limited to NPU clock domain)
    NpuInference,
    /// Network I/O completion handler
    NetworkIo,
}

impl TaskCategory {
    /// Maximum duration (kernel ticks) for this category.
    /// 1 tick ≈ 1ms. Foreground has no enforced limit (user controls it).
    pub fn max_duration_ticks(self) -> u64 {
        match self {
            Self::UserForeground   => u64::MAX,      // unlimited while focused
            Self::AudioRealtime    => 60_000,         // 60 seconds
            Self::BackgroundSync   => 30_000,         // 30 seconds
            Self::TimerCallback    => 5_000,          // 5 seconds
            Self::SystemCritical   => 120_000,        // 2 minutes
            Self::NpuInference     => 10_000,         // 10 seconds
            Self::NetworkIo        => 15_000,         // 15 seconds
        }
    }

    /// CPU share this category is entitled to (out of 1000).
    pub fn cpu_share(self) -> u32 {
        match self {
            Self::UserForeground  => 800,
            Self::AudioRealtime   => 600,
            Self::BackgroundSync  => 200,
            Self::TimerCallback   => 100,
            Self::SystemCritical  => 1000,
            Self::NpuInference    => 300,
            Self::NetworkIo       => 150,
        }
    }
}

// ── Active Task Token ─────────────────────────────────────────────────────────

/// A kernel-issued token granting a Silo the right to run at full power.
#[derive(Debug, Clone)]
pub struct ActiveTaskToken {
    /// Owning Silo ID
    pub silo_id: u64,
    /// Why this Silo needs to run
    pub category: TaskCategory,
    /// Human-readable reason (for Aether's Activity Monitor display)
    pub reason: String,
    /// Tick when this token was issued
    pub issued_at: u64,
    /// Tick when this token expires (MAX = indefinite for UserForeground)
    pub expires_at: u64,
    /// Number of times this token has auto-renewed
    pub renewal_count: u32,
    /// Maximum renewals before mandatory deep-sleep period
    pub max_renewals: u32,
}

impl ActiveTaskToken {
    pub fn new(silo_id: u64, category: TaskCategory, reason: &str, tick: u64) -> Self {
        let expires_at = tick.saturating_add(category.max_duration_ticks());
        ActiveTaskToken {
            silo_id,
            category,
            reason: reason.to_string(),
            issued_at: tick,
            expires_at,
            renewal_count: 0,
            max_renewals: match category {
                TaskCategory::BackgroundSync => 3,
                TaskCategory::TimerCallback  => 1,
                _                            => u32::MAX,
            },
        }
    }

    pub fn is_expired(&self, tick: u64) -> bool {
        tick > self.expires_at
    }

    pub fn can_renew(&self) -> bool {
        self.renewal_count < self.max_renewals
    }
}

// ── Energy Stats ──────────────────────────────────────────────────────────────

/// Law 8 enforcement statistics.
#[derive(Debug, Default, Clone)]
pub struct EnergyStats {
    /// Total tokens ever granted
    pub tokens_granted: u64,
    /// Tokens revoked due to expiry (enforced deep-sleep)
    pub tokens_expired: u64,
    /// Tokens explicitly released by app
    pub tokens_released: u64,
    /// Silos currently deep-sleeping (no active token)
    pub silos_deep_sleeping: u64,
    /// Law 8 violations (Silo tried to run without token)
    pub law8_violations: u64,
    /// Total Fibers currently deep-sleeping
    pub fibers_deep_sleeping: u64,
    /// Estimated milliwatts saved by deep-sleep enforcement
    pub mw_saved_estimate: u64,
}

// ── Silo Energy State ─────────────────────────────────────────────────────────

/// Current power state of a Silo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloPowerState {
    /// Running at full clock — holds an active ActiveTaskToken
    FullPower,
    /// Throttled — has a token but exceeded CPU share
    Throttled,
    /// Deep-sleeping — no token, Fibers suspended, stack swapped to NVMe
    DeepSleep,
    /// Waking — transitioning from DeepSleep to FullPower
    Waking,
}

#[derive(Debug, Clone)]
pub struct SiloEnergyRecord {
    pub silo_id: u64,
    pub state: SiloPowerState,
    pub token: Option<ActiveTaskToken>,
    /// Tick when state last changed
    pub state_changed_at: u64,
    /// Total ticks spent in DeepSleep (for reporting)
    pub deep_sleep_ticks: u64,
}

// ── Active Task Manager ───────────────────────────────────────────────────────

/// The kernel Law 8 enforcement engine.
pub struct ActiveTaskManager {
    /// Registry of all Silo energy records: silo_id → record
    pub registry: BTreeMap<u64, SiloEnergyRecord>,
    /// Global energy statistics
    pub stats: EnergyStats,
    /// Minimum deep-sleep period after BackgroundSync/Timer expiry (ticks)
    pub mandatory_sleep_ticks: u64,
}

impl ActiveTaskManager {
    pub fn new() -> Self {
        ActiveTaskManager {
            registry: BTreeMap::new(),
            stats: EnergyStats::default(),
            mandatory_sleep_ticks: 5_000, // 5 seconds mandatory rest
        }
    }

    /// Register a new Silo (called on spawn). Starts in DeepSleep (Law 1+8).
    pub fn register_silo(&mut self, silo_id: u64, tick: u64) {
        self.registry.insert(silo_id, SiloEnergyRecord {
            silo_id,
            state: SiloPowerState::DeepSleep,
            token: None,
            state_changed_at: tick,
            deep_sleep_ticks: 0,
        });
        self.stats.silos_deep_sleeping += 1;
        crate::serial_println!("[LAW8] Silo {} registered in DeepSleep.", silo_id);
    }

    /// Unregister a Silo (on vaporize).
    pub fn unregister_silo(&mut self, silo_id: u64) {
        if let Some(rec) = self.registry.remove(&silo_id) {
            if rec.state == SiloPowerState::DeepSleep {
                self.stats.silos_deep_sleeping = self.stats.silos_deep_sleeping.saturating_sub(1);
            }
        }
    }

    /// Grant an Active Task Token to a Silo.
    pub fn grant(&mut self, silo_id: u64, category: TaskCategory, reason: &str, tick: u64) -> bool {
        let record = self.registry.entry(silo_id).or_insert_with(|| SiloEnergyRecord {
            silo_id,
            state: SiloPowerState::DeepSleep,
            token: None,
            state_changed_at: tick,
            deep_sleep_ticks: 0,
        });

        let token = ActiveTaskToken::new(silo_id, category, reason, tick);
        let was_sleeping = record.state == SiloPowerState::DeepSleep;

        record.state = SiloPowerState::FullPower;
        record.token = Some(token);
        record.state_changed_at = tick;

        if was_sleeping {
            self.stats.silos_deep_sleeping = self.stats.silos_deep_sleeping.saturating_sub(1);
        }
        self.stats.tokens_granted += 1;

        crate::serial_println!(
            "[LAW8] Silo {} granted ActiveTask: {:?} — \"{}\" (cpu_share={})",
            silo_id, category, reason, category.cpu_share()
        );
        true
    }

    /// Explicitly release a token (app signals work is done).
    pub fn release(&mut self, silo_id: u64, tick: u64) {
        if let Some(record) = self.registry.get_mut(&silo_id) {
            let was_active = record.state == SiloPowerState::FullPower;
            record.token = None;
            record.state = SiloPowerState::DeepSleep;
            record.state_changed_at = tick;
            if was_active {
                self.stats.silos_deep_sleeping += 1;
                self.stats.tokens_released += 1;
            }
            crate::serial_println!("[LAW8] Silo {} released ActiveTask → DeepSleep.", silo_id);
        }
    }

    /// Law 8 enforcement tick — called by Sentinel every scheduler cycle.
    /// Checks for expired tokens and forces deep-sleep.
    pub fn enforce_tick(&mut self, tick: u64) -> Vec<u64> {
        let mut newly_sleeping: Vec<u64> = Vec::new();

        for record in self.registry.values_mut() {
            if record.state != SiloPowerState::FullPower { continue; }
            let expired = record.token.as_ref().map(|t| t.is_expired(tick)).unwrap_or(true);
            if expired {
                crate::serial_println!(
                    "[LAW8] Silo {} token EXPIRED → DeepSleep (Law 8).", record.silo_id
                );
                record.token = None;
                record.state = SiloPowerState::DeepSleep;
                record.state_changed_at = tick;
                self.stats.tokens_expired += 1;
                self.stats.silos_deep_sleeping += 1;
                // Estimate: deep-sleeping a Silo saves ~50mW
                self.stats.mw_saved_estimate += 50;
                newly_sleeping.push(record.silo_id);
            }
        }

        // Update deep_sleep_ticks for sleeping Silos
        for record in self.registry.values_mut() {
            if record.state == SiloPowerState::DeepSleep {
                record.deep_sleep_ticks += 1;
            }
        }

        newly_sleeping
    }

    /// Law 8 check: is this Silo allowed to execute a Fiber right now?
    pub fn check_law8(&self, silo_id: u64, tick: u64) -> bool {
        match self.registry.get(&silo_id) {
            Some(rec) if rec.state == SiloPowerState::FullPower => {
                rec.token.as_ref().map(|t| !t.is_expired(tick)).unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Get a Silo's current power state.
    pub fn power_state(&self, silo_id: u64) -> SiloPowerState {
        self.registry.get(&silo_id)
            .map(|r| r.state)
            .unwrap_or(SiloPowerState::DeepSleep)
    }

    /// Return CPU share (0-1000) for a Silo (used by scheduler).
    pub fn cpu_share(&self, silo_id: u64) -> u32 {
        self.registry.get(&silo_id)
            .and_then(|r| r.token.as_ref())
            .map(|t| t.category.cpu_share())
            .unwrap_or(0) // sleeping Silos get 0 CPU share
    }

    /// Print Law 8 enforcement summary.
    pub fn print_summary(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║  Law 8: Energy Proportionality       ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Active Silos:  {:>6}                 ║",
            self.registry.values().filter(|r| r.state == SiloPowerState::FullPower).count());
        crate::serial_println!("║ Sleeping Silos:{:>6}                 ║", self.stats.silos_deep_sleeping);
        crate::serial_println!("║ Tokens granted:{:>6}                 ║", self.stats.tokens_granted);
        crate::serial_println!("║ Tokens expired:{:>6} (Law 8 enforced)║", self.stats.tokens_expired);
        crate::serial_println!("║ Power saved:   {:>5}mW estimate       ║", self.stats.mw_saved_estimate);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
