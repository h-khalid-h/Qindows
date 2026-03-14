//! # Telemetry Bridge (Phase 125)
//!
//! ## Architecture Guardian: The Gap
//! `telemetry.rs` (Phase 84) implements `TelemetryEngine` with:
//! - `record()` — records a named metric value
//! - `record_silo()` — records per-Silo CPU/memory/net stats
//! - `check_alerts()` — returns silo IDs that exceeded alert thresholds
//! - `snapshot()` — exports current metric values
//!
//! **Missing link**: Nothing called `record_silo()` or `check_alerts()`.
//! The PMC loop, energy scheduler, and Q-Traffic engine all collect data
//! but never reported it to the telemetry engine for aggregation.
//!
//! This module provides `TelemetryBridge`:
//! 1. `collect_pmc_metrics()` — feeds PMC stats into TelemetryEngine
//! 2. `collect_energy_metrics()` — feeds EnergyScheduler P-state into telemetry
//! 3. `collect_traffic_metrics()` — feeds QTrafficEngine Law7 verdicts
//! 4. `check_and_report()` — calls check_alerts(), logs law violations
//! 5. `export_snapshot()` — structured export of all current metrics

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use crate::telemetry::{TelemetryEngine, SiloUsage};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TelemetryBridgeStats {
    pub collections: u64,
    pub alerts_fired: u64,
    pub silos_reported: u64,
    pub snapshots_exported: u64,
}

// ── Telemetry Bridge ──────────────────────────────────────────────────────────

/// Feeds live kernel subsystem data into TelemetryEngine.
pub struct TelemetryBridge {
    pub engine: TelemetryEngine,
    pub stats: TelemetryBridgeStats,
}

impl TelemetryBridge {
    pub fn new() -> Self {
        let mut engine = TelemetryEngine::new();
        // Register standard metrics
        engine.register("kernel.tick",          crate::telemetry::MetricCategory::Scheduler, "tick",    256);
        engine.register("silo.count",           crate::telemetry::MetricCategory::Silo,      "count",   128);
        engine.register("pmc.ipc",              crate::telemetry::MetricCategory::Cpu,       "ratio",   256);
        engine.register("pmc.cache_miss_pct",   crate::telemetry::MetricCategory::Memory,    "percent", 256);
        engine.register("energy.system_p_state",crate::telemetry::MetricCategory::Power,     "level",   128);
        engine.register("traffic.bytes_out",    crate::telemetry::MetricCategory::Network,   "bytes",   256);
        engine.register("law.violations",       crate::telemetry::MetricCategory::Silo,      "count",   128);
        engine.register("audit.checks",         crate::telemetry::MetricCategory::Silo,      "count",   256);
        TelemetryBridge { engine, stats: TelemetryBridgeStats::default() }
    }

    /// Feed PmcAnomalyLoop stats into telemetry (called from apic_tick_hook).
    pub fn collect_pmc_metrics(
        &mut self,
        silo_id: u64,
        instructions_retired: u64,
        cycles: u64,
        cache_misses: u64,
        cache_accesses: u64,
        tick: u64,
    ) {
        self.stats.collections += 1;
        self.stats.silos_reported += 1;

        let ipc = if cycles > 0 {
            (instructions_retired as f64 / cycles as f64) * 100.0
        } else { 0.0 };

        let miss_pct = if cache_accesses > 0 {
            (cache_misses as f64 / cache_accesses as f64) * 100.0
        } else { 0.0 };

        self.engine.record("pmc.ipc", ipc, tick);
        self.engine.record("pmc.cache_miss_pct", miss_pct, tick);

        self.engine.record_silo(
            silo_id,
            instructions_retired, // cpu_ns proxy
            cache_accesses * 64,  // memory proxy (cache lines)
            0, // storage_read
            0, // storage_write
        );
    }

    /// Feed EnergyScheduler P-state into telemetry.
    pub fn collect_energy_metrics(&mut self, p_state_level: u8, tick: u64) {
        self.engine.record("energy.system_p_state", p_state_level as f64, tick);
    }

    /// Feed QTrafficEngine total bytes out into telemetry.
    pub fn collect_traffic_metrics(&mut self, silo_id: u64, bytes_out: u64, tick: u64) {
        self.engine.record("traffic.bytes_out", bytes_out as f64, tick);
    }

    /// Record a law violation event.
    pub fn record_law_violation(&mut self, law: u8, tick: u64) {
        self.engine.record("law.violations", law as f64, tick);
    }

    /// Check alert thresholds, log, and return violated Silo IDs.
    pub fn check_and_report(&mut self) -> Vec<u64> {
        let violated = self.engine.check_alerts();
        if !violated.is_empty() {
            self.stats.alerts_fired += violated.len() as u64;
            for silo_id in &violated {
                crate::serial_println!(
                    "[TELEMETRY] Alert: Silo {} exceeded threshold", silo_id
                );
            }
        }
        violated
    }

    /// Export current snapshot as a formatted string.
    pub fn export_snapshot(&mut self, tick: u64) -> alloc::string::String {
        self.stats.snapshots_exported += 1;
        let snap = self.engine.snapshot();
        let mut out = alloc::string::String::from("=== Telemetry Snapshot ===\n");
        for (name, val) in &snap {
            out.push_str(&alloc::format!("  {:30} = {:.3}\n", name, val));
        }
        out.push_str(&alloc::format!("  tick = {}\n", tick));
        out
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  TelemetryBridge: collections={} alerts={} silos={} exports={}",
            self.stats.collections, self.stats.alerts_fired,
            self.stats.silos_reported, self.stats.snapshots_exported
        );
    }
}
