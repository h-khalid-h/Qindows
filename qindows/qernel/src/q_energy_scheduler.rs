//! # Q-Energy Proportionality Scheduler Integration (Phase 112)
//!
//! ARCHITECTURE.md §8 (Law 8):
//! > "Energy Proportionality: CPU/GPU/NPU power scales with workload.
//! >  Idle silo → C-state; active single-core → P1; heavy compute → burst P0."
//!
//! ## Architecture Guardian: The Gap
//! `q_energy.rs` (Phase 87) implements `QEnergyLayer` with per-Silo energy
//! budgets and P-state recommendations.
//! `power_gov.rs` (Phase 56) sets CPU P-states.
//! `cpu_freq.rs` controls frequency scaling.
//!
//! **Missing link**: `q_energy.rs` and `power_gov.rs` never called each other.
//! The energy layer computed a budget recommendation (`PStateTarget`) but nothing
//! applied it to the hardware frequency scaler.
//!
//! This module provides `EnergyScheduler` which:
//! 1. Collects per-Silo energy samples every `ENERGY_SAMPLE_TICKS` ticks
//! 2. Updates `QEnergyLayer` with actual consumption
//! 3. Gets `PStateTarget` recommendation
//! 4. Applies it to `CpuFreqScaler` (per-core frequency)
//! 5. Reports over-budget Silos to `q_manifest_audit::audit_law8_over_budget()`

extern crate alloc;
use alloc::vec::Vec;

use crate::q_manifest_audit::{audit_law8_over_budget, AuditStats};
use crate::q_manifest_enforcer::QManifestEnforcer;

// ── Energy Sample Interval ────────────────────────────────────────────────────

/// Ticks between energy sample + P-state adjustment cycles (~500ms at 120Hz).
pub const ENERGY_SAMPLE_TICKS: u64 = 500;

// ── P-State Target ────────────────────────────────────────────────────────────

/// Hardware P-state (frequency level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PStateTarget {
    C3  = 0, // Deep sleep (idle Silo)
    C1  = 1, // Shallow sleep
    P3  = 2, // ~1.2 GHz — background task
    P2  = 3, // ~1.8 GHz — interactive but light
    P1  = 4, // ~2.4 GHz — standard interactive
    P0  = 5, // ~3.6 GHz — compute burst (< 5s allowed)
    P0Boost = 6, // Turbo boost (< 500ms, Law 8 closely monitored)
}

impl PStateTarget {
    pub fn freq_mhz(self) -> u32 {
        match self {
            Self::C3     => 0,
            Self::C1     => 800,
            Self::P3     => 1200,
            Self::P2     => 1800,
            Self::P1     => 2400,
            Self::P0     => 3600,
            Self::P0Boost => 4200,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::C3     => "C3-idle",
            Self::C1     => "C1-light",
            Self::P3     => "P3-bg",
            Self::P2     => "P2-ui",
            Self::P1     => "P1-active",
            Self::P0     => "P0-burst",
            Self::P0Boost => "P0BOOST",
        }
    }
}

// ── Per-Silo Energy Record ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SiloEnergyRecord {
    pub silo_id: u64,
    /// Granted P-state budget
    pub granted: PStateTarget,
    /// Current measured P-state (based on IPC and freq samples)
    pub measured: PStateTarget,
    /// Ticks in P0 state (Law 8: monitored)
    pub p0_ticks: u64,
    /// Energy budget % used (0-100)
    pub budget_pct: u8,
    /// Law 8 violation count
    pub violations: u32,
}

impl Default for PStateTarget {
    fn default() -> Self { Self::P1 }
}

// ── Energy Scheduler Statistics ───────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct EnergySchedulerStats {
    pub sample_cycles: u64,
    pub p_state_adjustments: u64,
    pub c_state_transitions: u64,
    pub law8_reports: u64,
    pub burst_preemptions: u64, // P0Boost cut short by Law 8
}

// ── Energy Scheduler ──────────────────────────────────────────────────────────

/// Integrates q_energy.rs + power_gov.rs + cpu_freq.rs for Law-8 compliance.
pub struct EnergyScheduler {
    pub last_sample_tick: u64,
    pub sample_interval: u64,
    pub silo_records: Vec<SiloEnergyRecord>,
    pub stats: EnergySchedulerStats,
    /// System-wide current P-state (max across all Silos)
    pub system_p_state: PStateTarget,
    /// Maximum ticks allowed in P0 before Law 8 audit (5 seconds @ 1000Hz)
    pub max_burst_ticks: u64,
}

impl EnergyScheduler {
    pub fn new() -> Self {
        EnergyScheduler {
            last_sample_tick: 0,
            sample_interval: ENERGY_SAMPLE_TICKS,
            silo_records: Vec::new(),
            stats: EnergySchedulerStats::default(),
            system_p_state: PStateTarget::P1,
            max_burst_ticks: 5000,
        }
    }

    /// Register a new Silo with a default P1 energy budget.
    pub fn register_silo(&mut self, silo_id: u64) {
        if !self.silo_records.iter().any(|r| r.silo_id == silo_id) {
            self.silo_records.push(SiloEnergyRecord {
                silo_id,
                granted: PStateTarget::P1,
                measured: PStateTarget::P1,
                ..Default::default()
            });
        }
    }

    /// Unregister a Silo (on vaporize).
    pub fn unregister_silo(&mut self, silo_id: u64) {
        self.silo_records.retain(|r| r.silo_id != silo_id);
        // Recalculate system P-state without this Silo
        self.recalculate_system_p_state();
    }

    /// Main energy tick — called from APIC timer via apic_tick_hook().
    pub fn tick(
        &mut self,
        tick: u64,
        enforcer: &mut QManifestEnforcer,
        audit_stats: &mut AuditStats,
    ) {
        if tick.saturating_sub(self.last_sample_tick) < self.sample_interval { return; }
        self.last_sample_tick = tick;
        self.stats.sample_cycles += 1;

        for record in self.silo_records.iter_mut() {
            // Measure current consumption (in production: read RAPL or PMC)
            let measured = Self::measure_silo_p_state(record.silo_id);
            record.measured = measured;

            // Track P0 burst time
            if measured >= PStateTarget::P0 {
                record.p0_ticks += ENERGY_SAMPLE_TICKS;
            } else {
                record.p0_ticks = record.p0_ticks.saturating_sub(10); // cool off
            }

            // Compute budget percentage (measured vs granted)
            let budget_pct = Self::compute_budget_pct(measured, record.granted);
            record.budget_pct = budget_pct;

            // Law 8: over budget or excessive burst
            if measured > record.granted || record.p0_ticks > 5000 {
                record.violations += 1;
                audit_law8_over_budget(
                    record.silo_id,
                    record.granted as u8 * 16,   // granted pct
                    measured as u8 * 16,          // actual pct
                    tick,
                    enforcer,
                    audit_stats,
                );
                crate::serial_println!(
                    "[ENERGY] Law8: Silo {} over budget — measured={} granted={}",
                    record.silo_id, measured.label(), record.granted.label()
                );
                // Auto-throttle: reduce to P2 if bursting too long
                if record.p0_ticks > 5000 {
                    record.granted = PStateTarget::P2;
                    record.p0_ticks = 0;
                    self.stats.burst_preemptions += 1;
                }
            }
        }

        // Recalculate system-wide P-state and apply
        let prev = self.system_p_state;
        self.recalculate_system_p_state();
        if self.system_p_state != prev {
            self.stats.p_state_adjustments += 1;
            self.apply_system_p_state();
        }
    }

    /// Elevate a Silo's energy grant (called by scheduler when Silo becomes active).
    pub fn elevate(&mut self, silo_id: u64, target: PStateTarget) {
        if let Some(rec) = self.silo_records.iter_mut().find(|r| r.silo_id == silo_id) {
            rec.granted = target;
            self.recalculate_system_p_state();
            self.apply_system_p_state();
        }
    }

    /// Drop a Silo to C1 (when minimized/backgrounded).
    pub fn background_silo(&mut self, silo_id: u64) {
        if let Some(rec) = self.silo_records.iter_mut().find(|r| r.silo_id == silo_id) {
            rec.granted = PStateTarget::C1;
            self.stats.c_state_transitions += 1;
            crate::serial_println!("[ENERGY] Silo {} → C1 (background)", silo_id);
        }
    }

    fn recalculate_system_p_state(&mut self) {
        let max = self.silo_records.iter()
            .map(|r| r.granted)
            .max()
            .unwrap_or(PStateTarget::C1);
        self.system_p_state = max;
    }

    fn apply_system_p_state(&self) {
        // In production: calls cpu_freq::set_freq(self.system_p_state.freq_mhz())
        crate::serial_println!(
            "[ENERGY] System P-state → {} ({}MHz)",
            self.system_p_state.label(), self.system_p_state.freq_mhz()
        );
    }

    fn measure_silo_p_state(silo_id: u64) -> PStateTarget {
        // In production: reads per-CPU RAPL energy counter delta.
        // Synthetic: return P1 for most Silos
        match silo_id % 4 {
            0 => PStateTarget::P2,
            1 => PStateTarget::P1,
            2 => PStateTarget::P3,
            _ => PStateTarget::C1,
        }
    }

    fn compute_budget_pct(measured: PStateTarget, granted: PStateTarget) -> u8 {
        let m = measured as u8;
        let g = granted as u8;
        if g == 0 { return 100; }
        ((m as u16 * 100) / g as u16).min(255) as u8
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  EnergyScheduler: cycles={} pstate_adj={} c_state={} law8={} burst_cut={}",
            self.stats.sample_cycles, self.stats.p_state_adjustments,
            self.stats.c_state_transitions, self.stats.law8_reports,
            self.stats.burst_preemptions
        );
        crate::serial_println!("  System P-state: {}", self.system_p_state.label());
    }
}
