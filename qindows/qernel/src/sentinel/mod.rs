//! # The Sentinel — Kernel-Level AI Auditor
//!
//! An active observer that monitors the health, power consumption,
//! and capability usage of every Silo in real-time.
//!
//! Enforces the 10 Laws of the Q-Manifest at the hardware level.
//! If a Silo violates a law, the Sentinel revokes its scheduling
//! token within microseconds.

pub mod firewall;

use crate::silo::{QSilo, SiloState};

/// The 10 Laws of the Q-Manifest
#[derive(Debug, Clone, Copy)]
pub enum Law {
    ZeroAmbientAuthority,   // I
    ImmutableBinaries,      // II
    AsyncEverything,        // III
    VectorNativeUI,         // IV
    GlobalDeduplication,    // V
    SiloSandbox,            // VI
    TelemetryTransparency,  // VII
    EnergyProportionality,  // VIII
    UniversalNamespace,     // IX
    GracefulDegradation,    // X
}

/// Types of law violations
#[derive(Debug)]
pub enum Violation {
    /// Silo exceeded background CPU budget (Law VIII)
    EnergyDrain { cpu_percent: f32 },
    /// Silo blocked its main fiber for too long (Law III)
    SyncBlock { blocked_ms: u64 },
    /// Silo attempted unauthorized resource access (Law I)
    UnauthorizedAccess { resource: &'static str },
    /// Silo attempted to modify its own binary (Law II)
    SelfModification,
    /// Silo attempted silent network activity (Law VII)
    SilentTelemetry,
    /// Cache side-channel attack detected
    SideChannelAttack { cache_miss_rate: f32 },
    /// Memory leak detected (growing allocation without bounds)
    MemoryLeak { leaked_bytes: u64 },
}

/// Health report for a single Silo.
#[derive(Debug)]
pub struct HealthReport {
    pub silo_id: u64,
    pub cpu_usage_percent: f32,
    pub memory_usage_bytes: u64,
    pub thread_blocked_ms: u64,
    pub cache_miss_rate: f32,
    pub network_bytes_sent: u64,
    pub health_score: u8,
    pub violations: alloc::vec::Vec<Violation>,
}

use alloc::vec::Vec;

/// Sentinel configuration thresholds
pub struct SentinelConfig {
    /// Max CPU usage for background (non-focused) Silos
    pub max_background_cpu: f32,
    /// Max time a fiber can block synchronously before warning
    pub max_sync_block_ms: u64,
    /// Cache miss rate threshold for side-channel detection
    pub cache_miss_threshold: f32,
    /// Monitor cycle interval (in scheduler ticks)
    pub cycle_interval: u64,
}

impl Default for SentinelConfig {
    fn default() -> Self {
        SentinelConfig {
            max_background_cpu: 5.0,      // 5% max background CPU
            max_sync_block_ms: 16,        // 1 frame at 60Hz
            cache_miss_threshold: 80.0,   // Suspicious if > 80%
            cycle_interval: 1000,         // Every ~1ms
        }
    }
}

/// The Sentinel core.
pub struct Sentinel {
    pub config: SentinelConfig,
    pub total_violations: u64,
    pub silos_vaporized: u64,
}

impl Sentinel {
    pub fn new() -> Self {
        Sentinel {
            config: SentinelConfig::default(),
            total_violations: 0,
            silos_vaporized: 0,
        }
    }

    /// Analyze a Silo's behavior and produce a health report.
    ///
    /// Fix #3+4: CPU usage and block time are now computed from real per-silo
    /// tick counters instead of fixed zero / scaled-tick approximations.
    pub fn analyze(&self, silo: &QSilo) -> HealthReport {
        let mut violations = Vec::new();
        let mut score: u8 = 100;

        // ── Law VIII: Energy Proportionality ──────────────────────────────
        // Fix #4: compute CPU percentage from accumulated ticks vs cycle budget.
        // cycle_interval is in scheduler ticks (~1ms each).
        // cpu_ticks represents ticks spent executing during the last window.
        // Percentage = (ticks_used / cycle_ticks) * 100
        let ticks_per_cycle = self.config.cycle_interval.max(1) as f32;
        let cpu_usage = ((silo.cpu_ticks as f32) / ticks_per_cycle * 100.0).min(100.0);

        // Only flag background silos (silo ID > 1 means it's not the system silo)
        let is_background = silo.id > 1;
        if is_background && cpu_usage > self.config.max_background_cpu {
            violations.push(Violation::EnergyDrain {
                cpu_percent: cpu_usage,
            });
            score = score.saturating_sub(20);
        }

        // ── Law III: Asynchronous Everything ─────────────────────────────
        // Fix #3: compute blocked_ms from block_start_tick vs global scheduler tick.
        // If block_start_tick is non-zero, the silo is/was blocked.
        // The global tick counter is exposed by kstate::global_tick().
        let blocked_ms = if silo.block_start_tick > 0 {
            let now_tick = crate::kstate::global_tick();
            // Each tick ≈ 1 ms; compute elapsed ticks since block began.
            now_tick.saturating_sub(silo.block_start_tick)
        } else {
            0u64
        };

        if blocked_ms > self.config.max_sync_block_ms {
            violations.push(Violation::SyncBlock { blocked_ms });
            score = score.saturating_sub(15);
        }

        // ── Memory Leak Detection ─────────────────────────────────────────
        if silo.memory_used > silo.memory_limit {
            violations.push(Violation::MemoryLeak {
                leaked_bytes: silo.memory_used - silo.memory_limit,
            });
            score = score.saturating_sub(30);
        }

        HealthReport {
            silo_id: silo.id,
            cpu_usage_percent: cpu_usage,
            memory_usage_bytes: silo.memory_used,
            thread_blocked_ms: blocked_ms,
            cache_miss_rate: 0.0, // PMC-based in production
            network_bytes_sent: 0,
            health_score: score,
            violations,
        }
    }

    /// Enforce law violations — escalate based on severity.
    pub fn enforce(&mut self, silo: &mut QSilo, report: &HealthReport) {
        silo.health_score = report.health_score;

        for violation in &report.violations {
            self.total_violations += 1;

            match violation {
                Violation::SideChannelAttack { .. } | Violation::SelfModification => {
                    // Critical: immediate vaporization
                    crate::serial_println!(
                        "SENTINEL: CRITICAL violation in Silo {}. VAPORIZING.",
                        silo.id
                    );
                    silo.vaporize();
                    self.silos_vaporized += 1;
                }
                Violation::EnergyDrain { .. } => {
                    // Warning: throttle the Silo
                    crate::serial_println!(
                        "SENTINEL: Energy drain in Silo {}. Throttling.",
                        silo.id
                    );
                    silo.state = SiloState::Suspended;
                }
                Violation::SyncBlock { blocked_ms } => {
                    // Warning: dim the window via Aether
                    crate::serial_println!(
                        "SENTINEL: Silo {} blocked for {}ms. Dimming window.",
                        silo.id, blocked_ms
                    );
                }
                Violation::MemoryLeak { leaked_bytes } => {
                    // Warning then kill if persistent
                    crate::serial_println!(
                        "SENTINEL: Memory leak in Silo {} ({} bytes). Warning.",
                        silo.id, leaked_bytes
                    );
                    if silo.health_score < 30 {
                        silo.vaporize();
                        self.silos_vaporized += 1;
                    }
                }
                _ => {
                    crate::serial_println!(
                        "SENTINEL: Violation in Silo {}: {:?}",
                        silo.id, violation
                    );
                }
            }
        }
    }
}

/// Initialize the Sentinel on a dedicated CPU core.
pub fn init() {
    // In production: pin the Sentinel's monitor loop to Core 1
    // using the scheduler's core affinity system.
    crate::serial_println!("[OK] Sentinel AI Auditor initialized — Law Enforcement ACTIVE");
}
