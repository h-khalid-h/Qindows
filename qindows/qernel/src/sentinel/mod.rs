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
    // ── Phase 50: New hardware subsystem violations ────────────────────────
    /// Silo triggered excessive Copy-on-Write faults (Law II: Immutable Binaries abuse)
    /// High CoW fault rates can indicate a Silo is attempting to probe shared memory.
    CoWFault { fault_count: u64 },
    /// Silo attempted to steal or claim another Silo's IRQ vector (Law VI: Silo Sandbox)
    IrqVectorSteal { fault_count: usize },
    /// PCID pool is under pressure — risk of isolation breakdown (Law VI)
    /// Triggered when > 80% of all 4095 PCIDs are allocated simultaneously.
    PcidPoolPressure { allocated: u32, total: u32 },
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
    // ── Phase 50: Hardware subsystem fault counters ────────────────────────
    /// CoW page faults triggered by this Silo (from memory::cow)
    pub cow_faults: u64,
    /// IRQ vector fault attempts for this Silo (from irq_router)
    pub irq_faults: usize,
    /// System-wide PCID allocation pressure (0–100 percent)
    pub pcid_pressure_pct: u8,
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
            // Phase 50 fields — set to zero here; populated by analyze_extended()
            cow_faults: 0,
            irq_faults: 0,
            pcid_pressure_pct: 0,
        }
    }

    // ── Phase 50: Extended analysis with hardware subsystem fault data ────────

    /// Full analysis combining base metrics with Phase 48/49 hardware fault data.
    ///
    /// Called once per monitor cycle **after** `analyze()`. Merges CoW fault counts,
    /// IRQ Router fault counts, and PCID pressure into the report and updates the
    /// health score accordingly.
    ///
    /// ## Architecture Guardian: Separation of Concerns
    /// This function is deliberately separate from `analyze()` — it depends on
    /// `KernelState` (global singleton holding IrqRouter, CowManager, PcidAllocator)
    /// while `analyze()` depends only on the Silo itself. Keeping them separate
    /// prevents `analyze()` from gaining a dependency on the global state.
    pub fn analyze_extended(
        &self,
        report: &mut HealthReport,
        cow_faults_for_silo: u64,
        irq_faults_for_silo: usize,
    ) {
        // ── Phase 48: CoW Fault Rate (Law II: Immutable Binaries) ────────────
        // A small number of CoW faults is normal (Prism Ghost-Writes).
        // A spike suggests a Silo is probing shared pages to detect memory layout.
        const COW_FAULT_SPIKE_THRESHOLD: u64 = 50;
        report.cow_faults = cow_faults_for_silo;
        if cow_faults_for_silo > COW_FAULT_SPIKE_THRESHOLD {
            report.violations.push(Violation::CoWFault {
                fault_count: cow_faults_for_silo,
            });
            report.health_score = report.health_score.saturating_sub(25);
            crate::serial_println!(
                "SENTINEL: Silo {} CoW spike ({} faults) — possible memory probe.",
                report.silo_id, cow_faults_for_silo
            );
        }

        // ── Phase 49: IRQ Vector Steal attempts (Law VI: Silo Sandbox) ───────
        // Any non-zero IRQ fault count indicates an attempted vector steal or
        // missing CapToken — both are immediate Law I/VI violations.
        report.irq_faults = irq_faults_for_silo;
        if irq_faults_for_silo > 0 {
            report.violations.push(Violation::IrqVectorSteal {
                fault_count: irq_faults_for_silo,
            });
            // IRQ steal attempts are severe — immediately critical
            report.health_score = report.health_score.saturating_sub(40);
            crate::serial_println!(
                "SENTINEL: Silo {} made {} IRQ vector steal attempts — LAW VI VIOLATION.",
                report.silo_id, irq_faults_for_silo
            );
        }

        // ── Phase 48: PCID Pool Pressure (system-wide, Law VI) ───────────────
        // If >80% of PCID slots are taken, the risk of PCID recycling increases.
        // Recycled PCIDs before a full TLB flush could allow stale translations.
        let pcid_allocated = crate::memory::pcid::allocated_count();
        let pcid_total: u32 = 4095; // hardware maximum
        let pressure_pct = ((pcid_allocated as u64 * 100) / pcid_total as u64).min(100) as u8;
        report.pcid_pressure_pct = pressure_pct;

        if pressure_pct > 80 {
            report.violations.push(Violation::PcidPoolPressure {
                allocated: pcid_allocated,
                total: pcid_total,
            });
            // Pool pressure is a system-wide issue — don't penalise the Silo itself
            // but log it for the operator.
            crate::serial_println!(
                "SENTINEL: PCID pool at {}% ({}/{}) — isolation pressure HIGH.",
                pressure_pct, pcid_allocated, pcid_total
            );
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
                // ── Phase 50: New violation responses ────────────────────────
                Violation::IrqVectorSteal { fault_count } => {
                    // IRQ stealing is a deliberate sandbox escape attempt → vaporize
                    crate::serial_println!(
                        "SENTINEL: Silo {} made {} IRQ steal attempts. VAPORIZING — Law VI.",
                        silo.id, fault_count
                    );
                    silo.vaporize();
                    self.silos_vaporized += 1;
                }
                Violation::CoWFault { fault_count } => {
                    // CoW spike: suspend first, vaporize if score is critical
                    crate::serial_println!(
                        "SENTINEL: Silo {} CoW spike ({} faults). Suspending — Law II.",
                        silo.id, fault_count
                    );
                    silo.state = SiloState::Suspended;
                    if silo.health_score < 30 {
                        silo.vaporize();
                        self.silos_vaporized += 1;
                    }
                }
                Violation::PcidPoolPressure { allocated, total } => {
                    // System-wide pressure — do not kill the Silo (it's not at fault)
                    // Just log; the hypervisor layer should be alerted.
                    crate::serial_println!(
                        "SENTINEL: PCID pool at {}/{} — operator intervention advised.",
                        allocated, total
                    );
                }
                // ── Existing violation responses ──────────────────────────────
                Violation::EnergyDrain { .. } => {
                    crate::serial_println!(
                        "SENTINEL: Energy drain in Silo {}. Throttling.",
                        silo.id
                    );
                    silo.state = SiloState::Suspended;
                }
                Violation::SyncBlock { blocked_ms } => {
                    crate::serial_println!(
                        "SENTINEL: Silo {} blocked for {}ms. Dimming window.",
                        silo.id, blocked_ms
                    );
                }
                Violation::MemoryLeak { leaked_bytes } => {
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

    // ── Phase 50: Sentinel Snapshot (Telemetry Export) ────────────────────────

    /// Produce a lightweight snapshot of system-wide Sentinel state for telemetry.
    ///
    /// This snapshot is exported via the Q-Shell `sentinel status` command and
    /// via the `SyscallId::SentinelSnapshot` handler. It intentionally contains
    /// no per-Silo private data — only aggregate counters safe for telemetry.
    ///
    /// ## Q-Manifest Law 7: Telemetry Transparency
    /// All exported metrics are opt-in aggregate data. No individual Silo's
    /// memory content or code is ever included in a snapshot.
    pub fn snapshot(&self) -> SentinelSnapshot {
        SentinelSnapshot {
            total_violations: self.total_violations,
            silos_vaporized: self.silos_vaporized,
            pcid_allocated: crate::memory::pcid::allocated_count(),
            law_enforcement_active: true,
        }
    }
} // end impl Sentinel

/// Lightweight Sentinel telemetry snapshot for external export.
///
/// Contains ONLY aggregate counters with no per-Silo private data.
/// Exported via `Q-Shell sentinel status` and `SyscallId::SentinelSnapshot`.
///
/// ## Q-Manifest Law 7: Telemetry Transparency
/// This struct is the only telemetry surface of the Sentinel — nothing
/// deeper (HealthReports, Silo memory state) may be exposed externally.
#[derive(Debug, Clone)]
pub struct SentinelSnapshot {
    /// Total law violations detected since boot.
    pub total_violations: u64,
    /// Total Q-Silos vaporized by the Sentinel.
    pub silos_vaporized: u64,
    /// Current PCID allocations across all Silos (system-wide pressure gauge).
    pub pcid_allocated: u32,
    /// Whether the Sentinel's enforcement loop is currently active.
    pub law_enforcement_active: bool,
}

/// Initialize the Sentinel on a dedicated CPU core.
pub fn init() {
    // In production: pin the Sentinel's monitor loop to Core 1
    // using the scheduler's core affinity system.
    crate::serial_println!("[OK] Sentinel AI Auditor initialized — Law Enforcement ACTIVE");
}
