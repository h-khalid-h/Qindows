//! # Q-Metrics Bridge (Phase 130)
//!
//! ## Architecture Guardian: The Gap
//! `q_metrics.rs` (Phase 87) implements `QMetricsStore` with:
//! - `record(kind, value, tick)` — records any `MetricKind` value
//! - `benchmark_report()` — returns a `BenchmarkReport`
//! - 35+ `MetricKind` variants (IpcSendLatency, ContextSwitchNs, etc.)
//!
//! **Missing link**: Nothing ever called `QMetricsStore::record()` with
//! real values — not the scheduler, not the Q-Ring dispatch, not the
//! APIC timer. The benchmark_report() always returned zero-latency stats.
//!
//! This module provides `QMetricsBridge`:
//! 1. `on_ipc_send()` — records IPC send latency
//! 2. `on_context_switch()` — records scheduler context switch time
//! 3. `on_qring_submit()` — records Q-Ring submission round-trip
//! 4. `on_prism_write()` — records Prism ghost-write latency
//! 5. `on_syscall()` — records per-syscall latency in ns
//! 6. `report()` — formats a live BenchmarkReport for q_admin_bridge

extern crate alloc;
use alloc::string::String;
use alloc::format;

use crate::q_metrics::{QMetricsStore, MetricKind, BenchmarkReport};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct MetricsBridgeStats {
    pub ipc_records:        u64,
    pub context_switches:   u64,
    pub qring_submits:      u64,
    pub prism_writes:       u64,
    pub syscall_records:    u64,
    pub reports_generated:  u64,
}

// ── Q-Metrics Bridge ──────────────────────────────────────────────────────────

/// Feeds live kernel event timings into QMetricsStore.
pub struct QMetricsBridge {
    pub store: QMetricsStore,
    pub stats: MetricsBridgeStats,
}

impl QMetricsBridge {
    pub fn new(tick_freq_khz: u64) -> Self {
        QMetricsBridge {
            store: QMetricsStore::new(tick_freq_khz),
            stats: MetricsBridgeStats::default(),
        }
    }

    /// Record an IPC send latency (ticks from submit to completion).
    pub fn on_ipc_send(&mut self, latency_ticks: u64, tick: u64) {
        self.stats.ipc_records += 1;
        self.store.record(MetricKind::NetEgressBps, latency_ticks, tick); // IPC uses NetEgress metric
    }

    /// Record a context switch duration.
    pub fn on_context_switch(&mut self, duration_ticks: u64, tick: u64) {
        self.stats.context_switches += 1;
        self.store.record(MetricKind::ContextSwitchTicks, duration_ticks, tick);
    }

    /// Record a Q-Ring submission round-trip latency.
    pub fn on_qring_submit(&mut self, latency_ticks: u64, tick: u64) {
        self.stats.qring_submits += 1;
        self.store.record(MetricKind::QRingThroughput, latency_ticks, tick);
    }

    /// Record a Prism ghost-write latency.
    pub fn on_prism_write(&mut self, latency_ticks: u64, tick: u64) {
        self.stats.prism_writes += 1;
        self.store.record(MetricKind::NvmeWriteBps, latency_ticks, tick); // Prism write ~ NvmeWrite
    }

    /// Record a syscall latency by syscall number.
    pub fn on_syscall(&mut self, _syscall_nr: u16, latency_ticks: u64, tick: u64) {
        self.stats.syscall_records += 1;
        self.store.record(MetricKind::SyscallDispatchLatency, latency_ticks, tick);
    }

    /// Record a scheduler tick — used for scheduler stats.
    pub fn on_scheduler_tick(&mut self, runqueue_depth: u64, tick: u64) {
        self.store.record(MetricKind::SiloVmBytes, runqueue_depth, tick); // proxy
    }

    /// Record memory allocation latency.
    pub fn on_alloc(&mut self, latency_ticks: u64, tick: u64) {
        self.store.record(MetricKind::KernelHeapBytes, latency_ticks, tick); // proxy
    }

    /// Generate and return a formatted benchmark report.
    pub fn report(&mut self) -> BenchmarkReport {
        self.stats.reports_generated += 1;
        let report = self.store.benchmark_report();
        crate::serial_println!(
            "[METRICS] Benchmark: boot={}ms input={}µs ram={}MB spawn={}µs syscall_tp={}K ok={}",
            report.boot_time_ms, report.input_latency_us, report.idle_ram_mb,
            report.silo_spawn_us, report.syscall_throughput_k, report.all_targets_met
        );
        report
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  MetricsBridge: ipc={} ctx_sw={} qring={} prism={} syscalls={} reports={}",
            self.stats.ipc_records, self.stats.context_switches,
            self.stats.qring_submits, self.stats.prism_writes,
            self.stats.syscall_records, self.stats.reports_generated
        );
    }
}
