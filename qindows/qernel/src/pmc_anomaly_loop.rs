//! # PMC-Anomaly Integration Loop (Phase 110)
//!
//! ## Architecture Guardian: The Gap
//! `pmc.rs` reads raw hardware Performance Monitoring Counters (PMCs).
//! `sentinel_anomaly.rs` (Phase 90) scores PMC samples to detect threats.
//! `q_manifest_audit.rs` (Phase 107) calls the enforcer on violation.
//!
//! **Missing link**: the cycle:
//! ```text
//! pmc.rs → read counters per Silo
//!     → sentinel_anomaly.rs → score()
//!         → if alert: q_manifest_audit::audit_law*() → enforcer
//!             → EnforcementAction (warn/throttle/vaporize)
//!                 → kstate_ext::on_silo_vaporize() if needed
//! ```
//! This module implements the **full PMC → anomaly → enforcement loop**,
//! called at configurable intervals from the APIC timer.
//!
//! ## PMC Counter Mapping (per ARCHITECTURE.md §7)
//! InstructionsRetired, Cycles, CacheMisses, CacheAccesses,
//! BranchMispredicts, Branches, SyscallCount, NetBytesSent

extern crate alloc;
use alloc::vec::Vec;

use crate::sentinel_anomaly::{SentinelAnomalyScorer, PmcSample, AnomalyDimension};
use crate::q_manifest_audit::{audit_law1_cap_denied, audit_law6_isolation_breach, audit_law8_over_budget, audit_law3_blocking, AuditStats};
use crate::q_manifest_enforcer::QManifestEnforcer;

// ── PMC Loop Configuration ────────────────────────────────────────────────────

/// How many kernel ticks to skip between PMC scans (default: 100 = ~100ms).
pub const PMC_SCAN_INTERVAL_TICKS: u64 = 100;

// ── PMC Reading (hardware abstraction) ───────────────────────────────────────

/// Collect a `PmcSample` for the given Silo from hardware PMC registers.
/// In production: calls `rdpmc` or RDMSR IA32_PERFCTRx.
pub fn collect_sample(silo_id: u64, tick_duration: u64) -> PmcSample {
    // Synthetic counters that vary per Silo — replaced by real RDPMC calls
    let mix = silo_id.wrapping_mul(0x9E3779B9).wrapping_add(tick_duration);
    PmcSample {
        instructions_retired: mix & 0x0FFF_FFFF,
        cycles:               (mix >> 4) | 1,
        cache_misses:         (mix >> 8) & 0xFFFF,
        cache_accesses:       ((mix >> 8) & 0xFFFF) + 0x10000,
        branch_mispredicts:   (mix >> 12) & 0xFFF,
        branches:             ((mix >> 12) & 0xFFF) + 0x1000,
        syscall_count:        (mix >> 16) & 0xFF,
        net_bytes_sent:       (mix >> 20) & 0x3FFF,
        tick_duration:        tick_duration.max(1),
    }
}

// ── Enforcement Action Handler ────────────────────────────────────────────────

/// Map an AnomalyDimension to the appropriate Q-Manifest law audit call.
fn enforce_anomaly(
    silo_id: u64,
    dimension: AnomalyDimension,
    score: u8,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    audit_stats: &mut AuditStats,
) {
    match dimension {
        AnomalyDimension::CacheMiss => {
            // High L3 cache miss rate → memory scan / sandbox escape attempt
            if score > 85 {
                audit_law6_isolation_breach(silo_id, silo_id, 0xCA_CE_CA_CE, tick, enforcer, audit_stats);
            } else {
                audit_law8_over_budget(silo_id, 60, score, tick, enforcer, audit_stats);
            }
        }
        AnomalyDimension::BranchMiss | AnomalyDimension::IpcVariation => {
            // Branch misprediction / IPC variation → Spectre/Meltdown pattern
            // Report as Law 1 (capability hijack via speculative execution)
            audit_law1_cap_denied(silo_id, 0xFE, tick, enforcer, audit_stats);
            crate::serial_println!(
                "[PMC LOOP] Spectre-pattern detected: Silo {} score={} dim={:?}", silo_id, score, dimension
            );
        }
        AnomalyDimension::SyscallRate => {
            // Syscall rate spike → Law 3 (blocking/spin DoS)
            audit_law3_blocking(silo_id, score as u64 * 20, tick, enforcer, audit_stats);
        }
        AnomalyDimension::Ipc => {
            // IPC crash → possible sandbox violation or sleep-spin
            audit_law6_isolation_breach(silo_id, 0, score as u64, tick, enforcer, audit_stats);
        }
        AnomalyDimension::NetRate => {
            // Network bytes spike → Law 7 (unreported traffic / exfiltration)
            crate::q_manifest_audit::audit_law7_unreported_traffic(
                silo_id, score as u64 * 1024, tick, enforcer, audit_stats
            );
        }
        AnomalyDimension::Composite => {
            // Multiple dimensions: escalate to max severity (Law 6)
            audit_law6_isolation_breach(silo_id, 0, 0xBAD, tick, enforcer, audit_stats);
            crate::serial_println!(
                "[PMC LOOP] Composite anomaly: Silo {} score={} — escalating", silo_id, score
            );
        }
        AnomalyDimension::None => {}
    }
}

// ── PMC Loop Statistics ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct PmcLoopStats {
    pub scans: u64,
    pub samples_collected: u64,
    pub alerts_fired: u64,
    pub enforcements: u64,
}

// ── PMC-Anomaly Loop ──────────────────────────────────────────────────────────

/// Kernel main PMC monitoring loop.
/// Instantiated in `kstate_ext` and called from `boot_sequence::apic_tick_hook()`.
pub struct PmcAnomalyLoop {
    pub last_scan_tick: u64,
    pub scan_interval: u64,
    pub stats: PmcLoopStats,
}

impl PmcAnomalyLoop {
    pub fn new() -> Self {
        PmcAnomalyLoop {
            last_scan_tick: 0,
            scan_interval: PMC_SCAN_INTERVAL_TICKS,
            stats: PmcLoopStats::default(),
        }
    }

    /// Run one scan cycle for all known Silos (if interval elapsed).
    /// Called from APIC timer IRQ — must be fast.
    pub fn tick(
        &mut self,
        tick: u64,
        silo_ids: &[u64],
        anomaly: &mut SentinelAnomalyScorer,
        enforcer: &mut QManifestEnforcer,
        audit_stats: &mut AuditStats,
    ) {
        if tick.saturating_sub(self.last_scan_tick) < self.scan_interval { return; }
        let tick_duration = tick.saturating_sub(self.last_scan_tick);
        self.last_scan_tick = tick;
        self.stats.scans += 1;

        for &silo_id in silo_ids {
            let sample = collect_sample(silo_id, tick_duration);
            self.stats.samples_collected += 1;

            if let Some(score_result) = anomaly.score(silo_id, sample, tick) {
                if score_result.alert {
                    self.stats.alerts_fired += 1;
                    crate::serial_println!(
                        "[PMC LOOP] Alert: Silo {} score={} dim={:?}",
                        silo_id, score_result.score, score_result.primary_dimension
                    );
                    enforce_anomaly(
                        silo_id,
                        score_result.primary_dimension,
                        score_result.score,
                        tick,
                        enforcer,
                        audit_stats,
                    );
                    self.stats.enforcements += 1;
                }
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PmcAnomalyLoop: scans={} samples={} alerts={} enforcements={}",
            self.stats.scans, self.stats.samples_collected,
            self.stats.alerts_fired, self.stats.enforcements
        );
    }
}
