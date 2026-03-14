//! # Q-Manifest Law Runtime Audit (Phase 107)
//!
//! ## Architecture Guardian: The Gap
//! `q_manifest_enforcer.rs` (Phase 80) has `report_violation()` accepting
//! `LawViolationEvent`, and escalating through `EnforcementAction` responses.
//!
//! But **who calls `report_violation()`?** Currently:
//! - `sentinel/mod.rs` fires for law violations it directly detects
//! - `sentinel_anomaly.rs` fires for AI-detected threats
//!
//! Missing: **runtime hooks** that check each Q-Manifest law continuously
//! from the sources with the best visibility:
//!
//! | Law | Best monitoring source | Hook function |
//! |---|---|---|
//! | 1 Zero-Ambient Authority | qring_dispatch.rs (every syscall) | `audit_law1_cap_usage` |
//! | 2 Immutable Binaries | ledger.rs / silo_launch.rs (load time) | `audit_law2_binary_hash` |
//! | 3 Async Everything | qring_async drain stats (spin detection) | `audit_law3_blocking_detect` |
//! | 4 Vector-Native UI | chimera_vgdi_bridge stats (bitmap leakage) | `audit_law4_bitmap_leak` |
//! | 5 Global Deduplication | ghost_write_engine (duplicate chunk) | `audit_law5_dedup` |
//! | 6 Silo Sandbox | memory/paging.rs (cross-Silo address access) | `audit_law6_isolation` |
//! | 7 Telemetry Transparency | qtraffic.rs (unreported traffic) | `audit_law7_unreported` |
//! | 8 Energy Proportionality | q_energy.rs (over-budget silo) | `audit_law8_energy` |
//! | 9 Universal Namespace | uns_resolver (failed resolutions) | `audit_law9_uns_fail` |
//! | 10 Graceful Degradation | q_view_browser (offline mode) | `audit_law10_offline` |
//!
//! This module provides the **per-law audit functions** — thin adapters from
//! each subsystem's native data to `LawViolationEvent`.

extern crate alloc;
use alloc::string::ToString;
use alloc::format;

use crate::q_manifest_enforcer::{QManifestEnforcer, LawViolationEvent, ManifestLaw};

// ── Audit Statistics ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AuditStats {
    pub checks_per_law: [u64; 11],     // index = law number 1..10
    pub violations_per_law: [u64; 11],
    pub escalations: u64,
    pub vaporizations: u64,
}

// ── Per-Law Audit Functions ───────────────────────────────────────────────────

/// Law 1: Zero-Ambient Authority — unchecked capability usage.
/// Called from qring_dispatch when a syscall arrives without a valid CapToken.
pub fn audit_law1_cap_denied(
    silo_id: u64,
    opcode: u16,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[1] += 1;
    let action = enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::ZeroAmbientAuthority,
        silo_id,
        evidence: format!("Syscall opcode={:#x} attempted without CapToken", opcode),
        tick,
        binary_oid: None,
        severity_override: Some(70),
    });
    stats.violations_per_law[1] += 1;
    crate::serial_println!(
        "[MANIFEST AUDIT] Law1 violation: Silo {} opcode={:#x} → action={:?}", silo_id, opcode, action
    );
}

/// Law 2: Immutable Binaries — binary hash mismatch at load time.
pub fn audit_law2_binary_tampered(
    silo_id: u64,
    binary_oid: [u8; 32],
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[2] += 1;
    stats.violations_per_law[2] += 1;
    let action = enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::ImmutableBinaries,
        silo_id,
        evidence: format!(
            "Binary hash mismatch for OID {:02x}{:02x}.. at load time",
            binary_oid[0], binary_oid[1]
        ),
        tick,
        binary_oid: Some(binary_oid),
        severity_override: Some(95), // Always critical
    });
    crate::serial_println!(
        "[MANIFEST AUDIT] Law2 TAMPER detected: Silo {} OID={:02x}{:02x}.. → {:?}",
        silo_id, binary_oid[0], binary_oid[1], action
    );
    if matches!(action, crate::q_manifest_enforcer::EnforcementAction::VaporizeSilo) {
        stats.vaporizations += 1;
        crate::kstate_ext::on_silo_vaporize(silo_id, tick);
    }
}

/// Law 3: Async Everything — blocking kernel path detected.
/// Called when a Silo's Q-Ring hasn't drained for more than threshold ticks.
pub fn audit_law3_blocking(
    silo_id: u64,
    blocked_ticks: u64,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[3] += 1;
    // Only report if actually blocking (>1000 ticks = ~1 second)
    if blocked_ticks > 1000 {
        stats.violations_per_law[3] += 1;
        enforcer.report_violation(LawViolationEvent {
            law: ManifestLaw::AsyncEverything,
            silo_id,
            evidence: format!("Silo blocked Q-Ring drain for {}ticks", blocked_ticks),
            tick,
            binary_oid: None,
            severity_override: Some(50),
        });
        crate::serial_println!(
            "[MANIFEST AUDIT] Law3: Silo {} blocked {}ticks", silo_id, blocked_ticks
        );
    }
}

/// Law 4: Vector-Native UI — raw bitmap reaching compositor.
/// Called if chimera_vgdi_bridge fails to upscale (pixel pass-through detected).
pub fn audit_law4_bitmap_leaked(
    silo_id: u64,
    pixel_count: u32,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[4] += 1;
    stats.violations_per_law[4] += 1;
    enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::VectorNativeUi,
        silo_id,
        evidence: format!("{} raw pixels reached compositor without SDF conversion", pixel_count),
        tick,
        binary_oid: None,
        severity_override: Some(60),
    });
    crate::serial_println!(
        "[MANIFEST AUDIT] Law4: Silo {} {} pixel bitmap leaked to Aether", silo_id, pixel_count
    );
}

/// Law 5: Global Deduplication — duplicate content stored.
/// Called from ghost_write_engine when a duplicate chunk is written twice.
pub fn audit_law5_duplicate(
    silo_id: u64,
    chunk_hash: [u8; 32],
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[5] += 1;
    stats.violations_per_law[5] += 1;
    enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::GlobalDeduplication,
        silo_id,
        evidence: format!("Duplicate chunk {:02x}{:02x}.. written", chunk_hash[0], chunk_hash[1]),
        tick,
        binary_oid: None,
        severity_override: Some(30), // Low — dedup misses are not critical
    });
}

/// Law 6: Silo Sandbox — cross-Silo memory access attempt.
/// Called from memory/paging.rs on page fault with wrong CR3.
pub fn audit_law6_isolation_breach(
    offender_silo: u64,
    victim_silo: u64,
    fault_addr: u64,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[6] += 1;
    stats.violations_per_law[6] += 1;
    let action = enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::SiloSandbox,
        silo_id: offender_silo,
        evidence: format!(
            "Cross-Silo access: Silo {} touched Silo {} address {:#018x}",
            offender_silo, victim_silo, fault_addr
        ),
        tick,
        binary_oid: None,
        severity_override: Some(99), // Maximum — sandbox breach
    });
    crate::serial_println!(
        "[MANIFEST AUDIT] Law6 BREACH: Silo {} → Silo {} addr={:#x} → {:?}",
        offender_silo, victim_silo, fault_addr, action
    );
    // Always vaporize on sandbox breach
    stats.vaporizations += 1;
    crate::kstate_ext::on_silo_vaporize(offender_silo, tick);
}

/// Law 7: Telemetry Transparency — unreported network traffic.
/// Called when qtraffic.rs detects bytes sent without registering in traffic ledger.
pub fn audit_law7_unreported_traffic(
    silo_id: u64,
    bytes: u64,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[7] += 1;
    stats.violations_per_law[7] += 1;
    enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::TelemetryTransparency,
        silo_id,
        evidence: format!("{} bytes of network traffic not registered in Q-Traffic ledger", bytes),
        tick,
        binary_oid: None,
        severity_override: Some(60),
    });
}

/// Law 8: Energy Proportionality — Silo consuming above energy budget.
/// Called from q_energy.rs when a Silo's share exceeds its granted P-state slots.
pub fn audit_law8_over_budget(
    silo_id: u64,
    budget_pct: u8,
    actual_pct: u8,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[8] += 1;
    if actual_pct > budget_pct.saturating_add(10) {
        // Only enforce if >10% over budget (tolerate burst)
        stats.violations_per_law[8] += 1;
        enforcer.report_violation(LawViolationEvent {
            law: ManifestLaw::EnergyProportionality,
            silo_id,
            evidence: format!("Energy usage {}% exceeds budget {}%", actual_pct, budget_pct),
            tick,
            binary_oid: None,
            severity_override: Some(40),
        });
    }
}

/// Law 9: Universal Namespace — UNS resolution consistently failing.
pub fn audit_law9_uns_failure(
    silo_id: u64,
    uri: &str,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[9] += 1;
    stats.violations_per_law[9] += 1;
    enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::UniversalNamespace,
        silo_id,
        evidence: format!("UNS resolution failed: {}", uri),
        tick,
        binary_oid: None,
        severity_override: Some(20),
    });
}

/// Law 10: Graceful Degradation — app crashed instead of using offline cache.
pub fn audit_law10_offline_failure(
    silo_id: u64,
    tick: u64,
    enforcer: &mut QManifestEnforcer,
    stats: &mut AuditStats,
) {
    stats.checks_per_law[10] += 1;
    stats.violations_per_law[10] += 1;
    enforcer.report_violation(LawViolationEvent {
        law: ManifestLaw::GracefulDegradation,
        silo_id,
        evidence: "Silo crashed under connectivity loss instead of serving offline cache".to_string(),
        tick,
        binary_oid: None,
        severity_override: Some(50),
    });
}

/// Print all 10-law audit statistics.
pub fn print_audit_stats(stats: &AuditStats) {
    crate::serial_println!("  Q-Manifest Audit (per-law violations):");
    for i in 1..=10 {
        if stats.violations_per_law[i] > 0 {
            crate::serial_println!(
                "    Law {:2}: checks={} violations={}",
                i, stats.checks_per_law[i], stats.violations_per_law[i]
            );
        }
    }
    crate::serial_println!("    Vaporizations: {}", stats.vaporizations);
}
